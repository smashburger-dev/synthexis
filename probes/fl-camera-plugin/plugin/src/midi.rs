//! Process-wide CoreMIDI virtual source for write path D.
//!
//! Created lazily the first time path D is active and kept for the process
//! lifetime. Path D must work without a GuiContext, so this module never
//! touches plugin contexts: it builds a one-packet MIDI 1.0 UMP event list on
//! the stack and pushes it through `MIDIReceivedEventList`.

use std::ptr::NonNull;
use std::sync::OnceLock;

use objc2_core_foundation::CFString;
use objc2_core_midi::{
    MIDIClientCreate, MIDIClientRef, MIDIEndpointRef, MIDIEventList, MIDIEventPacket,
    MIDIProtocolID, MIDIReceivedEventList, MIDISourceCreateWithProtocol,
};
use objc2_foundation::NSString;

use crate::probe_log;

extern "C" {
    fn mach_absolute_time() -> u64;
}

static SOURCE: OnceLock<Result<MIDIEndpointRef, i32>> = OnceLock::new();

fn as_cf_string(s: &NSString) -> &CFString {
    // SAFETY: NSString and CFString are toll-free bridged.
    unsafe { &*(s as *const NSString).cast::<CFString>() }
}

fn create_source() -> Result<MIDIEndpointRef, i32> {
    unsafe {
        let client_name = NSString::from_str("synthexis-probe");
        let mut client: MIDIClientRef = 0;
        let status = MIDIClientCreate(
            as_cf_string(&client_name),
            None,
            std::ptr::null_mut(),
            NonNull::from(&mut client),
        );
        if status != 0 {
            probe_log::log(&format!("midi: MIDIClientCreate failed status={status}"));
            return Err(status);
        }
        let source_name = NSString::from_str("synthexis");
        let mut source: MIDIEndpointRef = 0;
        let status = MIDISourceCreateWithProtocol(
            client,
            as_cf_string(&source_name),
            MIDIProtocolID::Protocol_1_0,
            NonNull::from(&mut source),
        );
        if status != 0 {
            probe_log::log(&format!(
                "midi: MIDISourceCreateWithProtocol failed status={status}"
            ));
            return Err(status);
        }
        // The client and endpoint are opaque ids owned by the MIDI server;
        // they live for the process lifetime by design.
        probe_log::log("midi: virtual source \"synthexis\" created");
        Ok(source)
    }
}

/// Lazily create and return the process-wide virtual source. `None` if the
/// CoreMIDI calls failed (already logged with their OSStatus).
pub fn ensure_source() -> Option<MIDIEndpointRef> {
    SOURCE.get_or_init(create_source).as_ref().ok().copied()
}

/// Send one MIDI 1.0 control-change through the virtual source.
/// `channel` is 0-based (0 = channel 1), `cc` and `value` are 7-bit.
/// Returns the OSStatus from `MIDIReceivedEventList`.
pub fn send_cc(channel: u8, cc: u8, value: u8) -> i32 {
    let Some(source) = ensure_source() else {
        return -1;
    };
    // UMP MIDI 1.0 channel-voice message (message type 0x2), group 0:
    // status byte 0xBn, data1 = cc, data2 = value.
    let word = 0x2000_0000u32
        | (((0xB0 | (channel & 0x0F)) as u32) << 16)
        | (((cc & 0x7F) as u32) << 8)
        | (value & 0x7F) as u32;
    let mut list = MIDIEventList {
        protocol: MIDIProtocolID::Protocol_1_0,
        numPackets: 1,
        packet: [MIDIEventPacket {
            timeStamp: unsafe { mach_absolute_time() },
            wordCount: 1,
            words: {
                let mut words = [0u32; 64];
                words[0] = word;
                words
            },
        }],
    };
    unsafe { MIDIReceivedEventList(source, NonNull::from(&mut list)) }
}
