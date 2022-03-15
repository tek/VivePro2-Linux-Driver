use std::{io::Read, result};

use flate2::read::ZlibDecoder;
use hidapi::{HidApi, HidDevice, HidError};
use once_cell::sync::OnceCell;
use serde::Deserialize;
use serde_json::Value;

#[derive(thiserror::Error, Debug)]
pub enum Error {
	#[error("hid error: {0}")]
	Hid(#[from] HidError),
	#[error("device not found")]
	DeviceNotFound,
	#[error("device is not a vive device")]
	NotAVive,
	#[error("config size mismatch")]
	ConfigSizeMismatch,
	#[error("failed to read config")]
	ConfigReadFailed,
	#[error("protocol error: {0}")]
	ProtocolError(&'static str),
}

type Result<T, E = Error> = result::Result<T, E>;

static HIDAPI: OnceCell<HidApi> = OnceCell::new();
pub fn get_hidapi() -> Result<&'static HidApi> {
	HIDAPI.get_or_try_init(|| HidApi::new()).map_err(From::from)
}

const STEAM_VID: u16 = 0x28de;
const STEAM_PID: u16 = 0x2300;

#[derive(Deserialize, Debug)]
pub struct ConfigDevice {
	pub eye_target_height_in_pixels: u32,
	pub eye_target_width_in_pixels: u32,
}
#[derive(Deserialize, Debug)]
pub struct SteamConfig {
	pub device: ConfigDevice,
	pub direct_mode_edid_pid: u32,
	pub direct_mode_edid_vid: u32,
	pub seconds_from_photons_to_vblank: f64,
	pub seconds_from_vsync_to_photons: f64,
	/// SN of ViveDevice
	pub mb_serial_number: String,
}

pub struct SteamDevice(HidDevice);
impl SteamDevice {
	pub fn open_first() -> Result<Self> {
		let api = get_hidapi()?;
		let device = api.open(STEAM_VID, STEAM_PID)?;
		Ok(Self(device))
	}
	pub fn open(sn: &str) -> Result<Self> {
		let api = get_hidapi()?;
		let device = api
			.device_list()
			.find(|dev| dev.serial_number() == Some(sn))
			.ok_or(Error::DeviceNotFound)?;
		if device.vendor_id() != STEAM_VID || device.product_id() != STEAM_PID {
			return Err(Error::NotAVive);
		}
		let open = api.open_serial(device.vendor_id(), device.product_id(), sn)?;
		Ok(Self(open))
	}
	pub fn read_config(&self) -> Result<SteamConfig> {
		let mut report = [0u8; 64];
		report[0] = 16;
		let mut read_retries = 0;
		while self.0.get_feature_report(&mut report).is_err() {
			if read_retries > 5 {
				return Err(Error::ConfigReadFailed);
			}
			read_retries += 1;
		}
		read_retries = 0;
		let mut out = Vec::new();
		loop {
			report[0] = 17;
			if self.0.get_feature_report(&mut report).is_err() {
				if read_retries > 5 {
					return Err(Error::ConfigReadFailed);
				}
				read_retries += 1;
				continue;
			}
			read_retries = 0;
			if report[1] == 0 {
				break;
			}
			out.extend_from_slice(&report[2..2 + report[1] as usize])
		}
		let mut dec = ZlibDecoder::new(out.as_slice());
		let mut out = String::new();
		dec.read_to_string(&mut out)
			.map_err(|_| Error::ConfigReadFailed)?;

		serde_json::from_str(&out).map_err(|_| Error::ConfigReadFailed)
	}
}

const VIVE_VID: u16 = 0x0bb4;
const VIVE_PID: u16 = 0x0342;

#[derive(Deserialize, Debug)]
pub struct ViveConfig {
	pub device: ConfigDevice,
	pub direct_mode_edid_pid: u32,
	pub direct_mode_edid_vid: u32,
	pub seconds_from_photons_to_vblank: f64,
	pub seconds_from_vsync_to_photons: f64,
	/// Lets threat it as something opaque, anyway we directly feed this to lens-client
	pub inhouse_lens_correction: Value,
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Resolution {
	R2448x1224f90 = 0,
	R2448x1224f120 = 1,
	R3264x1632f90 = 2,
	R3680x1836f90 = 3,
	R4896x2448f90 = 4,
	R4896x2448f120 = 5,
}
impl Resolution {
	pub fn resolution(&self) -> (u32, u32) {
		match self {
			Self::R2448x1224f90 => (2448, 1224),
			Self::R2448x1224f120 => (2448, 1224),
			Self::R3264x1632f90 => (3264, 1632),
			Self::R3680x1836f90 => (3680, 1836),
			Self::R4896x2448f90 => (4896, 2448),
			Self::R4896x2448f120 => (4896, 2448),
		}
	}
	pub fn frame_rate(&self) -> f32 {
		match self {
			Self::R2448x1224f90 => 90.03,
			Self::R2448x1224f120 => 120.05,
			Self::R3264x1632f90 => 90.00,
			Self::R3680x1836f90 => 90.02,
			Self::R4896x2448f90 => 90.02,
			Self::R4896x2448f120 => 120.02,
		}
	}
}
impl TryFrom<u8> for Resolution {
	type Error = ();

	fn try_from(value: u8) -> Result<Self, Self::Error> {
		Ok(match value {
			0 => Self::R2448x1224f90,
			1 => Self::R2448x1224f120,
			2 => Self::R3264x1632f90,
			3 => Self::R3680x1836f90,
			4 => Self::R4896x2448f90,
			5 => Self::R4896x2448f120,
			_ => return Err(()),
		})
	}
}

pub struct ViveDevice(HidDevice);
impl ViveDevice {
	pub fn open_first() -> Result<Self> {
		let api = get_hidapi()?;
		let device = api.open(VIVE_VID, VIVE_PID)?;
		Ok(Self(device))
	}
	pub fn open(sn: &str) -> Result<Self> {
		let api = get_hidapi()?;
		let device = api
			.device_list()
			.find(|dev| dev.serial_number() == Some(sn))
			.ok_or(Error::DeviceNotFound)?;
		if device.vendor_id() != STEAM_VID || device.product_id() != STEAM_PID {
			return Err(Error::NotAVive);
		}
		let open = api.open_serial(device.vendor_id(), device.product_id(), sn)?;
		Ok(Self(open))
	}
	fn write(&self, id: u8, data: &[u8]) -> Result<()> {
		let mut report = [0u8; 64];
		report[0] = id;
		report[1..1 + data.len()].copy_from_slice(data);
		self.0.write(&report)?;
		Ok(())
	}
	fn write_feature(&self, id: u8, sub_id: u16, data: &[u8]) -> Result<()> {
		let mut report = [0u8; 64];
		report[0] = id;
		report[1] = (sub_id & 0xff) as u8;
		report[2] = (sub_id >> 8) as u8;
		report[3] = data.len() as u8;
		report[4..][..data.len()].copy_from_slice(data);
		self.0.send_feature_report(&report)?;
		Ok(())
	}
	fn read(&self, id: u8, strip_prefix: &[u8], out: &mut [u8]) -> Result<usize> {
		let mut data = [0u8; 64];
		self.0.read(&mut data)?;
		if data[0] != id {
			return Err(Error::ProtocolError("wrong report id"));
		}
		if &data[1..1 + strip_prefix.len()] != strip_prefix {
			return Err(Error::ProtocolError("wrong prefix"));
		}
		let size = data[1 + strip_prefix.len()] as usize;
		if size > 62 {
			return Err(Error::ProtocolError("wrong size"));
		}
		out[..size].copy_from_slice(&data[strip_prefix.len() + 2..strip_prefix.len() + 2 + size]);
		Ok(size)
	}
	pub fn read_devsn(&self) -> Result<String> {
		self.write(0x02, b"mfg-r-devsn")?;
		let mut out = [0u8; 62];
		let size = self.read(0x02, &[], &mut out)?;
		Ok(std::str::from_utf8(&out[..size])
			.map_err(|_| Error::ProtocolError("devsn is not a string"))?
			.to_string())
	}
	pub fn read_ipd(&self) -> Result<String> {
		self.write(0x02, b"mfg-r-ipdadc")?;
		let mut out = [0u8; 62];
		let size = self.read(0x02, &[], &mut out)?;
		Ok(std::str::from_utf8(&out[..size])
			.map_err(|_| Error::ProtocolError("devsn is not a string"))?
			.to_string())
	}
	pub fn read_config(&self) -> Result<ViveConfig> {
		let mut buf = [0u8; 62];
		// Request size
		let total_len = {
			self.write(0x01, &[0xea, 0xb1])?;
			let size = self.read(0x01, &[0xea, 0xb1], &mut buf)?;
			if size != 4 {
				return Err(Error::ProtocolError("config length has 4 bytes"));
			}
			let mut total_len = [0u8; 4];
			total_len.copy_from_slice(&buf[0..4]);
			u32::from_le_bytes(total_len) as usize
		};
		let mut read = 0;
		let mut out = Vec::<u8>::with_capacity(total_len);
		while read < total_len {
			let mut req = [0; 63];
			req[0] = 0xeb;
			req[1] = 0xb1;
			req[2] = 0x04;
			req[3..7].copy_from_slice(&u32::to_le_bytes(read as u32));

			self.write(0x01, &req)?;
			let size = self.read(0x01, &[0xeb, 0xb1], &mut buf)?;
			read += size;
			out.extend_from_slice(&buf[0..size]);
		}
		if read != total_len {
			return Err(Error::ProtocolError("config size mismatch"));
		}

		// First 128 bytes - something i can't decipher + sha256 hash (why?)
		let string = std::str::from_utf8(&out[128..])
			.map_err(|_| Error::ProtocolError("config is not utf-8"))?;

		serde_json::from_str(&string).map_err(|_| Error::ConfigReadFailed)
	}
	pub fn set_resolution(&self, resolution: Resolution) -> Result<(), Error> {
		self.write_feature(0x04, 0x2970, b"wireless,0")?;
		self.write_feature(0x04, 0x2970, format!("dtd,{}", resolution as u8).as_bytes())?;
		// TODO: wait for reconnection
		Ok(())
	}
}

#[test]
fn test() -> Result<()> {
	let dev = ViveDevice::open_first()?;
	dbg!(dev.read_ipd()?);
	Ok(())
}