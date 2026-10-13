//! The shared register model.
//!
//! The original keeps two flat arrays of 65536 u16 registers each — holding at
//! `*PTR_DAT_006c8b00 + 0xbc0` and input at `+0x20bc0` — and both the server and
//! the client read/write the same tables (manual §2.2: "server and client are
//! sharing registers"). We mirror that: one [`RegisterFile`] is shared by both
//! roles.

/// Number of addressable registers in each bank (addresses `0..=65535`).
pub const BANK_LEN: usize = 65536;

/// Which register bank an access targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank {
    /// Read/write (FC3, FC6, FC16, FC22, FC23).
    Holding,
    /// Read-only (FC4).
    Input,
}

/// Holding + input register banks, 65536 u16 each.
#[derive(Clone)]
pub struct RegisterFile {
    holding: Box<[u16]>,
    input: Box<[u16]>,
}

impl Default for RegisterFile {
    fn default() -> Self {
        Self::new()
    }
}

impl RegisterFile {
    pub fn new() -> Self {
        Self {
            holding: vec![0u16; BANK_LEN].into_boxed_slice(),
            input: vec![0u16; BANK_LEN].into_boxed_slice(),
        }
    }

    pub fn bank(&self, bank: Bank) -> &[u16] {
        match bank {
            Bank::Holding => &self.holding,
            Bank::Input => &self.input,
        }
    }

    pub fn get(&self, bank: Bank, addr: u16) -> u16 {
        self.bank(bank)[addr as usize]
    }

    /// Write a holding register. Input registers are read-only, so there is no
    /// `set` for them (matching the server, which only writes the holding bank).
    pub fn set_holding(&mut self, addr: u16, value: u16) {
        self.holding[addr as usize] = value;
    }

    /// Read a contiguous span `[addr, addr+count)`. Returns `None` if the span
    /// would run past the end of the bank — this is the `start + count <= 65536`
    /// guard the server enforces before touching the array.
    pub fn read_span(&self, bank: Bank, addr: u16, count: u16) -> Option<&[u16]> {
        let start = addr as usize;
        let end = start.checked_add(count as usize)?;
        if end > BANK_LEN {
            return None;
        }
        Some(&self.bank(bank)[start..end])
    }

    /// Initialize a bank the way the Options menu does (manual §2.7.3):
    /// every register set to 0, or to the value of its own address.
    pub fn initialize(&mut self, bank: Bank, mode: InitMode) {
        let arr = match bank {
            Bank::Holding => &mut self.holding,
            Bank::Input => &mut self.input,
        };
        match mode {
            InitMode::Zero => arr.iter_mut().for_each(|r| *r = 0),
            InitMode::Ascending => {
                for (i, r) in arr.iter_mut().enumerate() {
                    *r = i as u16;
                }
            }
        }
    }
}

/// Register initialization presets (Options → Initialize).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitMode {
    /// "All to '0'" (the startup default).
    Zero,
    /// "Ascending": register N holds value N.
    Ascending,
}
