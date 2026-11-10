//! TURZX / Turing V1.x USB protocol: the command packet and the USB transport.
//!
//! Facts from the protocol notes of libre-panel (GPL-3.0, docs/protocol/turzx-usb.md),
//! which were verified on a 9.2" panel:
//! - every command is 512 bytes: 500 plain bytes padded to 504, DES-CBC with key = IV,
//!   then the trailer `A1 1A` in the last two bytes;
//! - a payload (the PNG) follows the packet in the same bulk transfer;
//! - replies are 512 bytes and end with a zero-length packet, so read up to 1024.

use cbc::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
use std::time::Duration;

pub const VENDOR_ID: u16 = 0x1CBE;
pub const PRODUCT_ID: u16 = 0x0092;
const DES_KEY: &[u8; 8] = b"slv3tuzx";
const ACK: u8 = 0xC8;
const EP_OUT: u8 = 0x01;
const EP_IN: u8 = 0x81;
const MAX_PAYLOAD: usize = 1024 * 1024;

pub const CMD_SYNC: u8 = 10;
pub const CMD_BRIGHTNESS: u8 = 14;
pub const CMD_PNG: u8 = 102;

type DesCbcEnc = cbc::Encryptor<des::Des>;

pub fn build_packet(cmd: u8, args: &[u8], ms_since_midnight: u32) -> [u8; 512] {
    assert!(args.len() <= 492, "arguments too long");
    let mut packet = [0u8; 512];
    packet[0] = cmd;
    packet[2] = 0x1A;
    packet[3] = 0x6D;
    packet[4..8].copy_from_slice(&ms_since_midnight.to_le_bytes());
    packet[8..8 + args.len()].copy_from_slice(args);
    DesCbcEnc::new(DES_KEY.into(), DES_KEY.into())
        .encrypt_padded_mut::<NoPadding>(&mut packet[..504], 504)
        .expect("504 is a whole number of DES blocks");
    packet[510] = 0xA1;
    packet[511] = 0x1A;
    packet
}

pub struct Panel {
    handle: rusb::DeviceHandle<rusb::GlobalContext>,
}

impl Panel {
    pub fn open() -> Result<Panel, String> {
        let handle = rusb::open_device_with_vid_pid(VENDOR_ID, PRODUCT_ID).ok_or_else(|| {
            format!(
                "panel {VENDOR_ID:04x}:{PRODUCT_ID:04x} not found or not permitted \
                 (is the udev rule in place, and is another program holding it?)"
            )
        })?;
        let _ = handle.set_auto_detach_kernel_driver(true);
        handle
            .claim_interface(0)
            .map_err(|e| format!("cannot claim the panel's interface: {e}"))?;
        let panel = Panel { handle };
        panel.sync()?;
        Ok(panel)
    }

    /// Discard replies left over from an earlier session, then say hello.
    fn sync(&self) -> Result<(), String> {
        let mut last = String::new();
        for _ in 0..3 {
            let mut scratch = [0u8; 1024];
            for _ in 0..64 {
                if self
                    .handle
                    .read_bulk(EP_IN, &mut scratch, Duration::from_millis(50))
                    .is_err()
                {
                    break;
                }
            }
            match self.command(CMD_SYNC, &[], &[], 2000) {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(format!("panel does not answer the sync command: {last}"))
    }

    fn command(&self, cmd: u8, args: &[u8], payload: &[u8], timeout_ms: u64) -> Result<(), String> {
        let mut out = Vec::with_capacity(512 + payload.len());
        out.extend_from_slice(&build_packet(cmd, args, ms_since_midnight()));
        out.extend_from_slice(payload);
        let timeout = Duration::from_millis(timeout_ms);
        self.handle
            .write_bulk(EP_OUT, &out, timeout)
            .map_err(|e| format!("USB write, command {cmd}: {e}"))?;
        let mut reply = [0u8; 1024];
        let n = self
            .handle
            .read_bulk(EP_IN, &mut reply, timeout)
            .map_err(|e| format!("USB read, command {cmd}: {e}"))?;
        if n < 9 || reply[0] != cmd {
            return Err(format!("command {cmd}: unexpected reply {:02x?}", &reply[..n.min(16)]));
        }
        if reply[1] != ACK && reply[8] != ACK {
            return Err(format!("command {cmd} not accepted: {:02x?}", &reply[..16]));
        }
        Ok(())
    }

    /// 0-100 percent; the panel's own scale is 0-102.
    pub fn set_brightness(&self, percent: u8) -> Result<(), String> {
        let value = (percent.min(100) as u32 * 102 / 100) as u8;
        self.command(CMD_BRIGHTNESS, &[value], &[], 2000)
    }

    /// A whole frame as an RGBA PNG in the panel's native 480x1920 orientation.
    pub fn send_png(&self, png: &[u8]) -> Result<(), String> {
        if png.len() > MAX_PAYLOAD {
            return Err(format!("frame of {} KB is over the panel's 1 MiB limit", png.len() / 1024));
        }
        self.command(CMD_PNG, &(png.len() as u32).to_be_bytes(), png, 5000)
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(0);
    }
}

fn ms_since_midnight() -> u32 {
    let mut now: libc::timespec = unsafe { std::mem::zeroed() };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::clock_gettime(libc::CLOCK_REALTIME, &mut now);
        libc::localtime_r(&now.tv_sec, &mut tm);
    }
    let seconds = (tm.tm_hour as u32 * 60 + tm.tm_min as u32) * 60 + tm.tm_sec as u32;
    seconds * 1000 + (now.tv_nsec / 1_000_000) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference made with `openssl enc -des-cbc -nopad` over the same 504 plain bytes.
    #[test]
    fn sync_packet_matches_openssl() {
        let packet = build_packet(CMD_SYNC, &[], 0);
        assert_eq!(
            &packet[..16],
            &[
                0x3e, 0x7d, 0x4d, 0xc5, 0x07, 0xd9, 0xa2, 0x15, 0x6a, 0xf8, 0xc6, 0x61, 0xab, 0xa0,
                0xa3, 0x0e
            ]
        );
        assert_eq!(&packet[504..510], &[0u8; 6]);
        assert_eq!(&packet[510..], &[0xA1, 0x1A]);
    }

    #[test]
    fn arguments_change_the_cipher_text() {
        assert_ne!(build_packet(CMD_BRIGHTNESS, &[61], 0)[..504], build_packet(CMD_BRIGHTNESS, &[62], 0)[..504]);
    }
}
