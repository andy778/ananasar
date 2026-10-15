//! The DDEML server. It runs on a thread of its own, because DDEML delivers its
//! callbacks through the message loop of the thread that initialised it.
//!
//! This is the only module of the workspace that uses `unsafe` (see the crate
//! README). Each `unsafe` block is a single DDEML call whose arguments were
//! just produced or checked in safe code.

use crate::SERVICE;
use crate::item::{self, Activation};
use ananasar_core::registers::{Bank, RegisterFile};
use ananasar_core::sync::lock;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use windows_sys::Win32::System::DataExchange::{
    APPCLASS_STANDARD, CBF_FAIL_EXECUTES, CBF_SKIP_ALLNOTIFICATIONS, CP_WINUNICODE, DDE_FACK,
    DMLERR_NO_ERROR, DNS_REGISTER, DNS_UNREGISTER, DdeAccessData, DdeCreateDataHandle,
    DdeCreateStringHandleW, DdeFreeStringHandle, DdeInitializeW, DdeNameService, DdePostAdvise,
    DdeQueryStringW, DdeUnaccessData, DdeUninitialize, HCONV, HDDEDATA, HSZ, XTYP_ADVREQ,
    XTYP_ADVSTART, XTYP_CONNECT, XTYP_POKE, XTYP_REQUEST,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, KillTimer, MSG, PostThreadMessageW, SetTimer, TranslateMessage,
    WM_QUIT, WM_TIMER,
};

/// Clipboard format of plain text, the only format served.
const CF_TEXT: u32 = 1;
/// How often registers are compared to notify clients with a hot link.
const POLL_MS: u32 = 100;
/// The most advise notifications sent per poll; the rest follow on the next one.
const MAX_POSTS: usize = 512;

/// Why the DDE server did not start.
#[derive(Debug)]
pub enum StartError {
    /// A DDE server of this process is already running.
    AlreadyRunning,
    /// DDEML refused to initialise (its error code).
    Init(u32),
    /// DDEML refused to register the service name.
    Register,
    /// The server thread could not be started.
    Thread,
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => f.write_str("the DDE server is already running"),
            Self::Init(code) => write!(f, "DDE initialisation failed (error {code})"),
            Self::Register => f.write_str("cannot register the DDE service name"),
            Self::Thread => f.write_str("cannot start the DDE thread"),
        }
    }
}

impl std::error::Error for StartError {}

/// A running DDE server. Dropping it unregisters the service and stops the thread.
pub struct Server {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

impl Server {
    /// Registers the DDE service and serves the registers `activation` covers.
    pub fn start(
        regs: Arc<Mutex<RegisterFile>>,
        activation: Activation,
    ) -> Result<Self, StartError> {
        let (tx, rx) = mpsc::channel();
        let join = std::thread::Builder::new()
            .name("dde".into())
            .spawn(move || run(regs, activation, &tx))
            .map_err(|_| StartError::Thread)?;
        match rx.recv() {
            Ok(Ok(thread_id)) => Ok(Self {
                thread_id,
                join: Some(join),
            }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(e)
            }
            Err(_) => Err(StartError::Thread),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // SAFETY: plain call; posting WM_QUIT to a thread id is harmless even if that thread is gone.
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// What the DDEML callback needs; there is one server per process because the callback is a bare function.
#[derive(Clone)]
struct Active {
    regs: Arc<Mutex<RegisterFile>>,
    activation: Activation,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
static INSTANCE: AtomicU32 = AtomicU32::new(0);

fn handle(value: usize) -> HDDEDATA {
    ptr::without_provenance_mut(value)
}

fn null_handle() -> HDDEDATA {
    ptr::null_mut()
}

/// The text of a DDEML string handle.
fn query_string(hsz: HSZ) -> Option<String> {
    let mut buf = [0u16; 64];
    let cap = u32::try_from(buf.len()).ok()?;
    // SAFETY: `buf` is valid for `cap` UTF-16 units for the whole call.
    let n = unsafe {
        DdeQueryStringW(
            INSTANCE.load(Ordering::SeqCst),
            hsz,
            buf.as_mut_ptr(),
            cap,
            CP_WINUNICODE,
        )
    };
    String::from_utf16(buf.get(..usize::try_from(n).ok()?)?).ok()
}

/// The register a topic and item handle name, if the activation serves it.
fn target(active: &Active, topic: HSZ, item: HSZ) -> Option<(Bank, u16)> {
    let bank = item::parse_topic(&query_string(topic)?)?;
    let addr = item::parse_item(&query_string(item)?)?;
    active.activation.covers(bank, addr).then_some((bank, addr))
}

/// A CF_TEXT data handle holding the value of `bank` register `addr`.
fn value_data(active: &Active, bank: Bank, addr: u16, item: HSZ) -> HDDEDATA {
    let value = lock(&active.regs).get(bank, addr);
    let mut text = item::format_value(value).into_bytes();
    text.push(0);
    let Ok(len) = u32::try_from(text.len()) else {
        return null_handle();
    };
    // SAFETY: `text` is valid for `len` bytes; DDEML copies it.
    unsafe {
        DdeCreateDataHandle(
            INSTANCE.load(Ordering::SeqCst),
            text.as_ptr(),
            len,
            0,
            item,
            CF_TEXT,
            0,
        )
    }
}

/// Applies a poke; false if the item is not served or the data cannot be read.
fn poke(active: &Active, topic: HSZ, item: HSZ, data: HDDEDATA) -> bool {
    let Some((bank, addr)) = target(active, topic, item) else {
        return false;
    };
    let mut size = 0u32;
    // SAFETY: `data` is the handle DDEML passed to the callback.
    let p = unsafe { DdeAccessData(data, &mut size) };
    if p.is_null() {
        return false;
    }
    let len = usize::try_from(size).map_or(0, |n| n.min(64));
    // SAFETY: DDEML guarantees `size` readable bytes at `p` until DdeUnaccessData; `len` is no larger.
    let bytes = unsafe { std::slice::from_raw_parts(p, len) };
    let text = String::from_utf8_lossy(bytes).into_owned();
    // SAFETY: balances DdeAccessData above; `bytes` is not used after it.
    unsafe { DdeUnaccessData(data) };
    // Text that is not a number is acknowledged but changes nothing, like the original.
    if let Some(value) = item::parse_poke(&text) {
        lock(&active.regs).set(bank, addr, value);
    }
    true
}

/// The DDEML callback. A null result means "not processed".
unsafe extern "system" fn callback(
    wtype: u32,
    wfmt: u32,
    _hconv: HCONV,
    hsz1: HSZ,
    hsz2: HSZ,
    hdata: HDDEDATA,
    _data1: usize,
    _data2: usize,
) -> HDDEDATA {
    let Some(active) = lock(&ACTIVE).clone() else {
        return null_handle();
    };
    match wtype {
        // hsz1 is the topic, hsz2 the service.
        XTYP_CONNECT => {
            let service_ok = query_string(hsz2).is_some_and(|s| s.eq_ignore_ascii_case(SERVICE));
            let topic_ok = query_string(hsz1).is_some_and(|t| item::parse_topic(&t).is_some());
            if service_ok && topic_ok {
                handle(1)
            } else {
                null_handle()
            }
        }
        // hsz1 is the topic, hsz2 the item.
        XTYP_REQUEST | XTYP_ADVREQ if wfmt == CF_TEXT => match target(&active, hsz1, hsz2) {
            Some((bank, addr)) => value_data(&active, bank, addr, hsz2),
            None => null_handle(),
        },
        XTYP_ADVSTART if wfmt == CF_TEXT => {
            if target(&active, hsz1, hsz2).is_some() {
                handle(1)
            } else {
                null_handle()
            }
        }
        XTYP_POKE if wfmt == CF_TEXT => {
            if poke(&active, hsz1, hsz2, hdata) {
                handle(usize::try_from(DDE_FACK).unwrap_or(0))
            } else {
                null_handle()
            }
        }
        _ => null_handle(),
    }
}

/// The registers of one bank's served range as last seen, to find what changed.
struct Watch {
    bank: Bank,
    from: usize,
    last: Vec<u16>,
}

impl Watch {
    fn new(regs: &RegisterFile, bank: Bank, range: (u16, u16)) -> Self {
        let from = usize::from(range.0);
        let to = usize::from(range.1);
        let last = regs
            .bank(bank)
            .get(from..=to)
            .map(<[u16]>::to_vec)
            .unwrap_or_default();
        Self { bank, from, last }
    }

    /// Addresses whose value changed since the last call, at most `limit`.
    fn changed(&mut self, regs: &RegisterFile, limit: usize) -> Vec<u16> {
        let mut out = Vec::new();
        let Some(now) = regs
            .bank(self.bank)
            .get(self.from..self.from.saturating_add(self.last.len()))
        else {
            return out;
        };
        for (i, (old, new)) in self.last.iter_mut().zip(now).enumerate() {
            if old != new {
                if out.len() >= limit {
                    break;
                }
                *old = *new;
                if let Ok(addr) = u16::try_from(self.from.saturating_add(i)) {
                    out.push(addr);
                }
            }
        }
        out
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn string_handle(inst: u32, s: &str) -> HSZ {
    let w = wide(s);
    // SAFETY: `w` is NUL-terminated and outlives the call; DDEML copies it.
    unsafe { DdeCreateStringHandleW(inst, w.as_ptr(), CP_WINUNICODE) }
}

/// Tells DDEML that these registers changed, so clients with a hot link are updated.
fn post_advise(inst: u32, bank: Bank, addrs: &[u16]) {
    let topic = string_handle(inst, item::topic_name(bank));
    for &addr in addrs {
        let it = string_handle(inst, &item::item_name(addr));
        // SAFETY: both handles were created above for `inst` and are freed right after.
        unsafe {
            DdePostAdvise(inst, topic, it);
            DdeFreeStringHandle(inst, it);
        }
    }
    // SAFETY: the topic handle was created above for `inst`.
    unsafe { DdeFreeStringHandle(inst, topic) };
}

/// The body of the DDE thread: register, pump messages until WM_QUIT, unregister.
fn run(
    regs: Arc<Mutex<RegisterFile>>,
    activation: Activation,
    started: &mpsc::Sender<Result<u32, StartError>>,
) {
    {
        let mut slot = lock(&ACTIVE);
        if slot.is_some() {
            let _ = started.send(Err(StartError::AlreadyRunning));
            return;
        }
        *slot = Some(Active {
            regs: regs.clone(),
            activation,
        });
    }
    let result = serve(&regs, activation, started);
    *lock(&ACTIVE) = None;
    if let Err(e) = result {
        let _ = started.send(Err(e));
    }
}

fn serve(
    regs: &Arc<Mutex<RegisterFile>>,
    activation: Activation,
    started: &mpsc::Sender<Result<u32, StartError>>,
) -> Result<(), StartError> {
    let mut inst = 0u32;
    // SAFETY: `inst` outlives the call; `callback` has the PFNCALLBACK signature.
    let status = unsafe {
        DdeInitializeW(
            &mut inst,
            Some(callback),
            APPCLASS_STANDARD | CBF_SKIP_ALLNOTIFICATIONS | CBF_FAIL_EXECUTES,
            0,
        )
    };
    if status != DMLERR_NO_ERROR {
        return Err(StartError::Init(status));
    }
    INSTANCE.store(inst, Ordering::SeqCst);
    let service = string_handle(inst, SERVICE);
    // SAFETY: `service` was created for `inst` just above.
    let registered = !service.is_null()
        && !unsafe { DdeNameService(inst, service, ptr::null_mut(), DNS_REGISTER) }.is_null();
    if !registered {
        // SAFETY: tears down what was set up above.
        unsafe {
            DdeFreeStringHandle(inst, service);
            DdeUninitialize(inst);
        }
        return Err(StartError::Register);
    }

    let mut watches: Vec<Watch> = [Bank::Input, Bank::Holding]
        .into_iter()
        .filter_map(|bank| {
            activation
                .range(bank)
                .map(|range| Watch::new(&lock(regs), bank, range))
        })
        .collect();
    // SAFETY: a thread timer (no window) posts WM_TIMER to this thread's queue.
    let timer = unsafe { SetTimer(ptr::null_mut(), 0, POLL_MS, None) };
    // SAFETY: plain query of the calling thread's id.
    let _ = started.send(Ok(unsafe { GetCurrentThreadId() }));

    let mut msg = MSG::default();
    // SAFETY: `msg` outlives each call; GetMessageW returns 0 on WM_QUIT and -1 on error.
    while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0 {
        if msg.message == WM_TIMER && msg.hwnd.is_null() {
            for watch in &mut watches {
                let changed = watch.changed(&lock(regs), MAX_POSTS);
                if !changed.is_empty() {
                    post_advise(inst, watch.bank, &changed);
                }
            }
        } else {
            // SAFETY: `msg` was filled in by GetMessageW.
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    // SAFETY: tears down what was set up above, in reverse order.
    unsafe {
        KillTimer(ptr::null_mut(), timer);
        DdeNameService(inst, service, ptr::null_mut(), DNS_UNREGISTER);
        DdeFreeStringHandle(inst, service);
        DdeUninitialize(inst);
    }
    INSTANCE.store(0, Ordering::SeqCst);
    Ok(())
}
