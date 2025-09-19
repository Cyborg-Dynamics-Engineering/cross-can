///
/// can.rs
///
/// Provides an abstracted CanFrame data struct.
///
///
///
use bytemuck::{Pod, Zeroable};

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanFrame {
    id: u32,
    dlc: u8,
    flags: u8, // bit 0 = extended, bit 1 = rtr, bit 2 = error
    timestamp: u64,
    data: [u8; 8],
}

unsafe impl Pod for CanFrame {}
unsafe impl Zeroable for CanFrame {}

impl CanFrame {
    const FLAG_EXTENDED: u8 = 0b0000_0001;
    const FLAG_RTR: u8 = 0b0000_0010;
    const FLAG_ERROR: u8 = 0b0000_0100;

    /// Create a new Standard ID CAN data frame
    pub fn new(id: u32, data: &[u8]) -> Result<Self, &'static str> {
        Self::validate_id(id, false)?;
        Self::validate_data(data)?;
        let mut buf = [0u8; 8];
        buf[..data.len()].copy_from_slice(data);
        Ok(Self {
            id,
            dlc: data.len() as u8,
            flags: 0,
            timestamp: 0,
            data: buf,
        })
    }

    /// Create a new Extended ID CAN data frame
    pub fn new_eff(id: u32, data: &[u8]) -> Result<Self, &'static str> {
        Self::validate_id(id, true)?;
        Self::validate_data(data)?;
        let mut buf = [0u8; 8];
        buf[..data.len()].copy_from_slice(data);
        Ok(Self {
            id,
            dlc: data.len() as u8,
            flags: Self::FLAG_EXTENDED,
            timestamp: 0,
            data: buf,
        })
    }

    /// Create a new CAN remote frame
    pub fn new_remote(id: u32, dlc: usize, is_extended: bool) -> Result<Self, &'static str> {
        if dlc > 8 {
            return Err("RTR frame DLC must be <= 8");
        }
        Self::validate_id(id, is_extended)?;
        let mut flags = Self::FLAG_RTR;
        if is_extended {
            flags |= Self::FLAG_EXTENDED;
        }
        Ok(Self {
            id,
            dlc: dlc as u8,
            flags,
            timestamp: 0,
            data: [0u8; 8],
        })
    }

    /// Create a new CAN error frame
    pub fn new_error(id: u32) -> Result<Self, &'static str> {
        if id > 0x1FFFFFFF {
            return Err("CAN error frame ID must be <= 29 bits");
        }
        Ok(Self {
            id,
            dlc: 0,
            flags: Self::FLAG_ERROR,
            timestamp: 0,
            data: [0u8; 8],
        })
    }

    pub fn set_timestamp(&mut self, ts: Option<u64>) {
        self.timestamp = ts.unwrap_or(0);
    }

    pub fn timestamp(&self) -> Option<u64> {
        if self.timestamp == 0 {
            None
        } else {
            Some(self.timestamp)
        }
    }

    fn validate_id(id: u32, extended: bool) -> Result<(), &'static str> {
        if extended {
            if id > 0x1FFFFFFF {
                return Err("Extended ID must be <= 29 bits (0x1FFFFFFF)");
            }
        } else {
            if id > 0x7FF {
                return Err("Standard ID must be <= 11 bits (0x7FF)");
            }
        }
        Ok(())
    }

    fn validate_data(data: &[u8]) -> Result<(), &'static str> {
        if data.len() > 8 {
            Err("CAN data must be <= 8 bytes")
        } else {
            Ok(())
        }
    }

    // --- Getters ---

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn data(&self) -> &[u8] {
        &self.data[..self.dlc as usize]
    }

    pub fn dlc(&self) -> usize {
        self.dlc as usize
    }

    pub fn is_extended(&self) -> bool {
        (self.flags & Self::FLAG_EXTENDED) != 0
    }

    pub fn is_rtr(&self) -> bool {
        (self.flags & Self::FLAG_RTR) != 0
    }

    pub fn is_error(&self) -> bool {
        (self.flags & Self::FLAG_ERROR) != 0
    }
}

#[cfg(target_os = "linux")]
impl From<socketcan::CanFrame> for CanFrame {
    fn from(sc: socketcan::CanFrame) -> Self {
        use socketcan::{self, EmbeddedFrame, Frame};

        let id_raw = match sc.id() {
            socketcan::Id::Standard(standard_id) => standard_id.as_raw() as u32,
            socketcan::Id::Extended(extended_id) => extended_id.as_raw(),
        };

        if sc.is_remote_frame() {
            return CanFrame::new_remote(id_raw, sc.data().len(), sc.is_extended()).unwrap();
        }
        if sc.is_error_frame() {
            return CanFrame::new_error(id_raw).unwrap();
        }
        if sc.is_extended() {
            return CanFrame::new_eff(id_raw, sc.data()).unwrap();
        } else {
            return CanFrame::new(id_raw, sc.data()).unwrap();
        }
    }
}

#[cfg(target_os = "linux")]
impl Into<socketcan::CanFrame> for CanFrame {
    fn into(self) -> socketcan::CanFrame {
        use socketcan::{self, EmbeddedFrame};

        let sc_id = if self.is_extended() {
            match socketcan::ExtendedId::new(self.id()) {
                Some(ext_id) => Ok(socketcan::Id::Extended(ext_id)),
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Invalid CAN ID for extended can frame: {:?}", self.id()),
                )),
            }
        } else {
            match socketcan::StandardId::new(self.id() as u16) {
                Some(std_id) => Ok(socketcan::Id::Standard(std_id)),
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Invalid CAN ID for standard can frame: {:?}", self.id()),
                )),
            }
        }
        .unwrap();

        if self.is_error() {
            return socketcan::CanFrame::Error(
                socketcan::CanErrorFrame::new_error(self.id(), self.data()).unwrap(),
            );
        }
        if self.is_rtr() {
            return socketcan::CanFrame::Remote(
                socketcan::CanRemoteFrame::new(sc_id, self.data()).unwrap(),
            );
        }

        socketcan::CanFrame::Data(socketcan::CanDataFrame::new(sc_id, self.data()).unwrap())
    }
}
