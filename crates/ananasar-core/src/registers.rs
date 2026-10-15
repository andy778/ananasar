//! The shared register model.
//!
//! Two flat arrays of 65536 u16 registers each (holding + input); both the
//! server and the client read/write the same tables. One [`RegisterFile`] is shared by both roles.

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
    /// A register file with every register zeroed.
    pub fn new() -> Self {
        Self {
            holding: vec![0u16; BANK_LEN].into_boxed_slice(),
            input: vec![0u16; BANK_LEN].into_boxed_slice(),
        }
    }

    /// All registers of `bank`.
    pub fn bank(&self, bank: Bank) -> &[u16] {
        match bank {
            Bank::Holding => &self.holding,
            Bank::Input => &self.input,
        }
    }

    /// Read one register.
    pub fn get(&self, bank: Bank, addr: u16) -> u16 {
        // The banks hold one word per u16 address, so the lookup cannot miss.
        self.bank(bank).get(usize::from(addr)).copied().unwrap_or(0)
    }

    /// Write a holding register. Input registers are read-only, so there is no
    /// `set` for them (matching the server, which only writes the holding bank).
    pub fn set_holding(&mut self, addr: u16, value: u16) {
        if let Some(r) = self.holding.get_mut(usize::from(addr)) {
            *r = value;
        }
    }

    /// Write an input register. Clients cannot do this over Modbus; it exists
    /// for loading a saved register file and for the application's own editing.
    pub fn set_input(&mut self, addr: u16, value: u16) {
        if let Some(r) = self.input.get_mut(usize::from(addr)) {
            *r = value;
        }
    }

    /// Write a register in either bank (see [`RegisterFile::set_input`]).
    pub fn set(&mut self, bank: Bank, addr: u16, value: u16) {
        match bank {
            Bank::Holding => self.set_holding(addr, value),
            Bank::Input => self.set_input(addr, value),
        }
    }

    /// Read a contiguous span `[addr, addr+count)`. Returns `None` if the span
    /// would run past the end of the bank — this is the `start + count <= 65536`
    /// guard the server enforces before touching the array.
    pub fn read_span(&self, bank: Bank, addr: u16, count: u16) -> Option<&[u16]> {
        let start = usize::from(addr);
        let end = start.checked_add(usize::from(count))?;
        self.bank(bank).get(start..end)
    }

    /// Initialize a bank the way the original's Options menu does:
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
                    *r = u16::try_from(i).unwrap_or(u16::MAX);
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
