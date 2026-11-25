//! Application-layer ("MDR") messages: building requests and parsing the
//! replies / notifications sent back by the headset.
//!
//! Sony uses two generations of this protocol:
//!
//! * **V1** – WH-1000XM3, WF-1000XM3, WH-1000XM4 (4-byte init reply).
//! * **V2** – WH-1000XM5/XM6, WF-1000XM4/XM5, LinkBuds, ... (8-byte init reply).
//!
//! Many opcodes differ between them (in V2, `0x13` is the codec reply, while in
//! V1 it is a battery notification), so every encoder and parser takes the
//! [`Dialect`] negotiated with the headset.
//!
//! Byte layouts follow Gadgetbridge's open-source implementation
//! (`SonyProtocolImplV1` / `SonyProtocolImplV2`). Every parser is
//! bounds-checked: a malformed packet produces an error, never a panic.

use std::fmt;

// ---------------------------------------------------------------------------
// Dialect
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ProtocolVersion {
    #[default]
    V1,
    V2,
}

/// Sub-type used by V2 ambient-sound messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AmbientType {
    /// `0x15`: noise cancelling / ambient only (e.g. WH-1000XM5).
    #[default]
    Basic,
    /// `0x17`: adds wind noise reduction (e.g. WF-1000XM5).
    WithWind,
}

impl AmbientType {
    fn code(self) -> u8 {
        match self {
            AmbientType::Basic => 0x15,
            AmbientType::WithWind => 0x17,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            0x15 => Some(AmbientType::Basic),
            0x17 => Some(AmbientType::WithWind),
            _ => None,
        }
    }

    pub fn other(self) -> Self {
        match self {
            AmbientType::Basic => AmbientType::WithWind,
            AmbientType::WithWind => AmbientType::Basic,
        }
    }
}

/// Everything needed to encode / decode messages for a particular headset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Dialect {
    pub version: ProtocolVersion,
    pub ambient: AmbientType,
}

impl Dialect {
    pub const V1: Dialect = Dialect {
        version: ProtocolVersion::V1,
        ambient: AmbientType::Basic,
    };

    pub fn v2(ambient: AmbientType) -> Self {
        Dialect {
            version: ProtocolVersion::V2,
            ambient,
        }
    }

    pub fn is_v2(self) -> bool {
        self.version == ProtocolVersion::V2
    }

    pub fn supports_wind_reduction(self) -> bool {
        !self.is_v2() || self.ambient == AmbientType::WithWind
    }

    pub fn supports_touch_sensor(self) -> bool {
        !self.is_v2()
    }

    pub fn auto_power_off_options(self) -> &'static [AutoPowerOff] {
        if self.is_v2() {
            &[AutoPowerOff::Off, AutoPowerOff::WhenTakenOff]
        } else {
            &[
                AutoPowerOff::Off,
                AutoPowerOff::After5Min,
                AutoPowerOff::After30Min,
                AutoPowerOff::After1Hour,
                AutoPowerOff::After3Hours,
            ]
        }
    }
}

/// Opcodes (first payload byte).
pub mod op {
    pub const INIT_REQUEST: u8 = 0x00;
    pub const INIT_REPLY: u8 = 0x01;
    pub const FW_VERSION_GET: u8 = 0x04;
    pub const FW_VERSION_RET: u8 = 0x05;

    pub const V1_BATTERY_GET: u8 = 0x10;
    pub const V1_BATTERY_RET: u8 = 0x11;
    pub const V1_BATTERY_NOTIFY: u8 = 0x13;
    pub const V1_CODEC_GET: u8 = 0x18;
    pub const V1_CODEC_RET: u8 = 0x19;
    pub const V1_CODEC_NOTIFY: u8 = 0x1B;
    pub const V1_POWER_OFF: u8 = 0x22;

    pub const V2_CODEC_GET: u8 = 0x12;
    pub const V2_CODEC_RET: u8 = 0x13;
    pub const V2_CODEC_NOTIFY: u8 = 0x15;
    pub const V2_BATTERY_GET: u8 = 0x22;
    pub const V2_BATTERY_RET: u8 = 0x23;
    pub const V2_POWER_SET: u8 = 0x24;
    pub const V2_BATTERY_NOTIFY: u8 = 0x25;
    pub const V2_AUTO_POWER_OFF_GET: u8 = 0x26;
    pub const V2_AUTO_POWER_OFF_RET: u8 = 0x27;
    pub const V2_AUTO_POWER_OFF_SET: u8 = 0x28;
    pub const V2_AUTO_POWER_OFF_NOTIFY: u8 = 0x29;

    pub const EQ_GET: u8 = 0x56;
    pub const EQ_RET: u8 = 0x57;
    pub const EQ_SET: u8 = 0x58;
    pub const EQ_NOTIFY: u8 = 0x59;
    pub const AMBIENT_GET: u8 = 0x66;
    pub const AMBIENT_RET: u8 = 0x67;
    pub const AMBIENT_SET: u8 = 0x68;
    pub const AMBIENT_NOTIFY: u8 = 0x69;
    pub const TOUCH_SENSOR_GET: u8 = 0xD6;
    pub const TOUCH_SENSOR_RET: u8 = 0xD7;
    pub const TOUCH_SENSOR_SET: u8 = 0xD8;
    pub const TOUCH_SENSOR_NOTIFY: u8 = 0xD9;
    pub const AUDIO_UPSAMPLING_GET: u8 = 0xE6;
    pub const AUDIO_UPSAMPLING_RET: u8 = 0xE7;
    pub const AUDIO_UPSAMPLING_SET: u8 = 0xE8;
    pub const AUDIO_UPSAMPLING_NOTIFY: u8 = 0xE9;
    /// "Automatic power off / button mode" family: also carries
    /// Speak-to-Chat on/off and pause-when-taken-off.
    pub const SYSTEM_GET: u8 = 0xF6;
    pub const SYSTEM_RET: u8 = 0xF7;
    pub const SYSTEM_SET: u8 = 0xF8;
    pub const SYSTEM_NOTIFY: u8 = 0xF9;
    pub const STC_CONFIG_GET: u8 = 0xFA;
    pub const STC_CONFIG_RET: u8 = 0xFB;
    pub const STC_CONFIG_SET: u8 = 0xFC;
    pub const STC_CONFIG_NOTIFY: u8 = 0xFD;
}

/// Sub-types of the `SYSTEM_*` family.
mod sys {
    pub const V1_PAUSE: u8 = 0x03;
    pub const V1_AUTO_POWER_OFF: u8 = 0x04;
    pub const V1_STC: u8 = 0x05;
    pub const V2_PAUSE: u8 = 0x01;
    pub const V2_STC: u8 = 0x0C;
    pub const V2_AUTO_POWER_OFF: u8 = 0x05;
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AmbientMode {
    #[default]
    NoiseCancelling,
    WindReduction,
    AmbientSound,
    Off,
}

/// Noise cancelling / ambient sound state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmbientSound {
    pub mode: AmbientMode,
    /// Ambient sound level, 0..=20 (the UI uses 1..=20).
    pub level: u8,
    pub focus_on_voice: bool,
}

impl Default for AmbientSound {
    fn default() -> Self {
        Self {
            mode: AmbientMode::NoiseCancelling,
            level: 20,
            focus_on_voice: false,
        }
    }
}

pub const AMBIENT_LEVEL_MIN: u8 = 1;
pub const AMBIENT_LEVEL_MAX: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryKind {
    /// Over-ear headphones (single battery).
    Single,
    /// True-wireless earbuds (left + right).
    Dual,
    /// Alternative left/right battery query used by some V2 earbuds.
    Dual2,
    /// Charging case of true-wireless earbuds.
    Case,
}

impl BatteryKind {
    fn code(self, d: Dialect) -> u8 {
        match (self, d.version) {
            (BatteryKind::Single, _) => 0x00,
            (BatteryKind::Dual, ProtocolVersion::V1) => 0x01,
            (BatteryKind::Dual2, ProtocolVersion::V1) => 0x01,
            (BatteryKind::Case, ProtocolVersion::V1) => 0x02,
            (BatteryKind::Dual, ProtocolVersion::V2) => 0x09,
            (BatteryKind::Dual2, ProtocolVersion::V2) => 0x01,
            (BatteryKind::Case, ProtocolVersion::V2) => 0x0A,
        }
    }

    fn from_code(code: u8, d: Dialect) -> Option<Self> {
        match (code, d.version) {
            (0x00, _) => Some(BatteryKind::Single),
            (0x01, ProtocolVersion::V1) => Some(BatteryKind::Dual),
            (0x02, ProtocolVersion::V1) => Some(BatteryKind::Case),
            (0x01, ProtocolVersion::V2) => Some(BatteryKind::Dual2),
            (0x09, ProtocolVersion::V2) => Some(BatteryKind::Dual),
            (0x0A, ProtocolVersion::V2) => Some(BatteryKind::Case),
            _ => None,
        }
    }

    fn is_dual(self) -> bool {
        matches!(self, BatteryKind::Dual | BatteryKind::Dual2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Battery {
    pub level: u8,
    pub charging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EqPreset {
    Off,
    Bright,
    Excited,
    Mellow,
    Relaxed,
    Vocal,
    TrebleBoost,
    BassBoost,
    Speech,
    Manual,
    Custom1,
    Custom2,
    Unknown(u8),
}

impl EqPreset {
    pub const ALL: [EqPreset; 12] = [
        EqPreset::Off,
        EqPreset::Bright,
        EqPreset::Excited,
        EqPreset::Mellow,
        EqPreset::Relaxed,
        EqPreset::Vocal,
        EqPreset::TrebleBoost,
        EqPreset::BassBoost,
        EqPreset::Speech,
        EqPreset::Manual,
        EqPreset::Custom1,
        EqPreset::Custom2,
    ];

    pub fn code(self) -> u8 {
        match self {
            EqPreset::Off => 0x00,
            EqPreset::Bright => 0x10,
            EqPreset::Excited => 0x11,
            EqPreset::Mellow => 0x12,
            EqPreset::Relaxed => 0x13,
            EqPreset::Vocal => 0x14,
            EqPreset::TrebleBoost => 0x15,
            EqPreset::BassBoost => 0x16,
            EqPreset::Speech => 0x17,
            EqPreset::Manual => 0xA0,
            EqPreset::Custom1 => 0xA1,
            EqPreset::Custom2 => 0xA2,
            EqPreset::Unknown(c) => c,
        }
    }

    pub fn from_code(code: u8) -> Self {
        EqPreset::ALL
            .into_iter()
            .find(|p| p.code() == code)
            .unwrap_or(EqPreset::Unknown(code))
    }

    pub fn label(self) -> &'static str {
        match self {
            EqPreset::Off => "Off",
            EqPreset::Bright => "Bright",
            EqPreset::Excited => "Excited",
            EqPreset::Mellow => "Mellow",
            EqPreset::Relaxed => "Relaxed",
            EqPreset::Vocal => "Vocal",
            EqPreset::TrebleBoost => "Treble Boost",
            EqPreset::BassBoost => "Bass Boost",
            EqPreset::Speech => "Speech",
            EqPreset::Manual => "Manual",
            EqPreset::Custom1 => "Custom 1",
            EqPreset::Custom2 => "Custom 2",
            EqPreset::Unknown(_) => "Unknown",
        }
    }

    /// Whether the user can edit the bands of this preset.
    pub fn is_editable(self) -> bool {
        matches!(
            self,
            EqPreset::Manual | EqPreset::Custom1 | EqPreset::Custom2
        )
    }

    /// Approximate band curve of the built-in presets (used to preview them
    /// until the headset reports the real curve, and by the simulator).
    pub fn default_bands(self) -> [i8; 6] {
        match self {
            EqPreset::Bright => [0, -1, 0, 2, 4, 5],
            EqPreset::Excited => [2, 3, 0, -1, 2, 4],
            EqPreset::Mellow => [1, 2, 1, 0, -2, -4],
            EqPreset::Relaxed => [0, -1, -2, -3, -4, -5],
            EqPreset::Vocal => [-2, -1, 3, 4, 2, 0],
            EqPreset::TrebleBoost => [0, 0, 0, 2, 5, 8],
            EqPreset::BassBoost => [6, 4, 1, 0, 0, 0],
            EqPreset::Speech => [-4, 0, 4, 4, 1, -2],
            _ => [0; 6],
        }
    }
}

/// Equalizer: preset + Clear Bass + 5 bands (400 Hz, 1 kHz, 2.5 kHz,
/// 6.3 kHz, 16 kHz). Every value is in the range -10..=10.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Equalizer {
    pub preset: EqPreset,
    /// `[clear_bass, 400, 1k, 2.5k, 6.3k, 16k]`
    pub bands: [i8; 6],
}

impl Default for Equalizer {
    fn default() -> Self {
        Self {
            preset: EqPreset::Off,
            bands: [0; 6],
        }
    }
}

pub const EQ_BAND_LABELS: [&str; 6] = ["CLEAR\nBASS", "400", "1k", "2.5k", "6.3k", "16k"];
pub const EQ_MIN: i8 = -10;
pub const EQ_MAX: i8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Sbc,
    Aac,
    Ldac,
    AptX,
    AptXHd,
    Lc3,
    Unknown(u8),
}

impl Codec {
    pub fn from_code(code: u8) -> Self {
        match code {
            0x01 => Codec::Sbc,
            0x02 => Codec::Aac,
            0x10 => Codec::Ldac,
            0x20 => Codec::AptX,
            0x21 => Codec::AptXHd,
            0x30 => Codec::Lc3,
            c => Codec::Unknown(c),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Codec::Sbc => 0x01,
            Codec::Aac => 0x02,
            Codec::Ldac => 0x10,
            Codec::AptX => 0x20,
            Codec::AptXHd => 0x21,
            Codec::Lc3 => 0x30,
            Codec::Unknown(c) => c,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Codec::Sbc => "SBC",
            Codec::Aac => "AAC",
            Codec::Ldac => "LDAC",
            Codec::AptX => "aptX",
            Codec::AptXHd => "aptX HD",
            Codec::Lc3 => "LC3",
            Codec::Unknown(_) => "—",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AutoPowerOff {
    #[default]
    Off,
    WhenTakenOff,
    After5Min,
    After30Min,
    After1Hour,
    After3Hours,
}

impl AutoPowerOff {
    pub const ALL: [AutoPowerOff; 6] = [
        AutoPowerOff::Off,
        AutoPowerOff::WhenTakenOff,
        AutoPowerOff::After5Min,
        AutoPowerOff::After30Min,
        AutoPowerOff::After1Hour,
        AutoPowerOff::After3Hours,
    ];

    fn codes(self) -> [u8; 2] {
        match self {
            AutoPowerOff::Off => [0x11, 0x00],
            AutoPowerOff::WhenTakenOff => [0x10, 0x00],
            AutoPowerOff::After5Min => [0x00, 0x00],
            AutoPowerOff::After30Min => [0x01, 0x01],
            AutoPowerOff::After1Hour => [0x02, 0x02],
            AutoPowerOff::After3Hours => [0x03, 0x03],
        }
    }

    fn from_codes(a: u8, b: u8) -> Option<Self> {
        AutoPowerOff::ALL.into_iter().find(|x| x.codes() == [a, b])
    }

    pub fn label(self) -> &'static str {
        match self {
            AutoPowerOff::Off => "Do not turn off",
            AutoPowerOff::WhenTakenOff => "When headphones are taken off",
            AutoPowerOff::After5Min => "After 5 minutes",
            AutoPowerOff::After30Min => "After 30 minutes",
            AutoPowerOff::After1Hour => "After 1 hour",
            AutoPowerOff::After3Hours => "After 3 hours",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StcSensitivity {
    #[default]
    Auto,
    High,
    Low,
}

impl StcSensitivity {
    pub const ALL: [StcSensitivity; 3] = [
        StcSensitivity::Auto,
        StcSensitivity::High,
        StcSensitivity::Low,
    ];
    fn code(self) -> u8 {
        match self {
            StcSensitivity::Auto => 0,
            StcSensitivity::High => 1,
            StcSensitivity::Low => 2,
        }
    }
    fn from_code(c: u8) -> Option<Self> {
        StcSensitivity::ALL.into_iter().find(|x| x.code() == c)
    }
    pub fn label(self) -> &'static str {
        match self {
            StcSensitivity::Auto => "Auto",
            StcSensitivity::High => "High",
            StcSensitivity::Low => "Low",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StcTimeout {
    Short,
    #[default]
    Standard,
    Long,
    Never,
}

impl StcTimeout {
    pub const ALL: [StcTimeout; 4] = [
        StcTimeout::Short,
        StcTimeout::Standard,
        StcTimeout::Long,
        StcTimeout::Never,
    ];
    fn code(self) -> u8 {
        match self {
            StcTimeout::Short => 0,
            StcTimeout::Standard => 1,
            StcTimeout::Long => 2,
            StcTimeout::Never => 3,
        }
    }
    fn from_code(c: u8) -> Option<Self> {
        StcTimeout::ALL.into_iter().find(|x| x.code() == c)
    }
    pub fn label(self) -> &'static str {
        match self {
            StcTimeout::Short => "Short (approx. 15 sec.)",
            StcTimeout::Standard => "Standard (approx. 30 sec.)",
            StcTimeout::Long => "Long (approx. 1 min.)",
            StcTimeout::Never => "Do not end automatically",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SpeakToChatConfig {
    pub sensitivity: StcSensitivity,
    /// Only configurable on V1 headsets.
    pub focus_on_voice: bool,
    pub timeout: StcTimeout,
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// A command sent to the headset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Init,
    GetFirmwareVersion,
    GetBattery(BatteryKind),
    GetCodec,
    GetAmbientSound,
    /// `adjusting = true` while a slider is still being dragged (V1 only).
    SetAmbientSound {
        value: AmbientSound,
        adjusting: bool,
    },
    GetEqualizer,
    /// Select a preset; the headset restores that preset's bands.
    SetEqualizerPreset(EqPreset),
    /// Set the bands of an editable preset.
    SetEqualizerBands(Equalizer),
    GetAudioUpsampling,
    SetAudioUpsampling(bool),
    GetSpeakToChat,
    SetSpeakToChat(bool),
    GetSpeakToChatConfig,
    SetSpeakToChatConfig(SpeakToChatConfig),
    GetTouchSensor,
    SetTouchSensor(bool),
    GetPauseWhenTakenOff,
    SetPauseWhenTakenOff(bool),
    GetAutoPowerOff,
    SetAutoPowerOff(AutoPowerOff),
    PowerOff,
}

impl Request {
    /// Encode the request for the given dialect. `None` means the headset
    /// generation has no such command.
    pub fn payload(&self, d: Dialect) -> Option<Vec<u8>> {
        let v2 = d.is_v2();
        Some(match self {
            Request::Init => vec![op::INIT_REQUEST, 0x00],
            Request::GetFirmwareVersion => vec![op::FW_VERSION_GET, 0x02],
            Request::GetBattery(kind) => {
                let opcode = if v2 {
                    op::V2_BATTERY_GET
                } else {
                    op::V1_BATTERY_GET
                };
                vec![opcode, kind.code(d)]
            }
            Request::GetCodec => {
                if v2 {
                    vec![op::V2_CODEC_GET, 0x02]
                } else {
                    vec![op::V1_CODEC_GET, 0x00]
                }
            }
            Request::GetAmbientSound => {
                vec![op::AMBIENT_GET, if v2 { d.ambient.code() } else { 0x02 }]
            }
            Request::SetAmbientSound { value, adjusting } => {
                ambient_payload(op::AMBIENT_SET, value, *adjusting, d)
            }
            Request::GetEqualizer => vec![op::EQ_GET, 0x00],
            Request::SetEqualizerPreset(p) => vec![op::EQ_SET, 0x00, p.code(), 0x00],
            Request::SetEqualizerBands(eq) => eq_payload(op::EQ_SET, eq),
            Request::GetAudioUpsampling => {
                vec![op::AUDIO_UPSAMPLING_GET, if v2 { 0x01 } else { 0x02 }]
            }
            Request::SetAudioUpsampling(on) => {
                if v2 {
                    vec![op::AUDIO_UPSAMPLING_SET, 0x01, u8::from(*on)]
                } else {
                    vec![op::AUDIO_UPSAMPLING_SET, 0x02, 0x00, u8::from(*on)]
                }
            }
            Request::GetSpeakToChat => {
                vec![op::SYSTEM_GET, if v2 { sys::V2_STC } else { sys::V1_STC }]
            }
            Request::SetSpeakToChat(on) => {
                if v2 {
                    // V2 sends a "disabled" flag.
                    vec![op::SYSTEM_SET, sys::V2_STC, u8::from(!*on), 0x01]
                } else {
                    vec![op::SYSTEM_SET, sys::V1_STC, 0x01, u8::from(*on)]
                }
            }
            Request::GetSpeakToChatConfig => vec![op::STC_CONFIG_GET, if v2 { 0x0C } else { 0x05 }],
            Request::SetSpeakToChatConfig(cfg) => {
                if v2 {
                    vec![
                        op::STC_CONFIG_SET,
                        0x0C,
                        cfg.sensitivity.code(),
                        cfg.timeout.code(),
                    ]
                } else {
                    vec![
                        op::STC_CONFIG_SET,
                        0x05,
                        0x00,
                        cfg.sensitivity.code(),
                        u8::from(cfg.focus_on_voice),
                        cfg.timeout.code(),
                    ]
                }
            }
            Request::GetTouchSensor if !v2 => vec![op::TOUCH_SENSOR_GET, 0xD2],
            Request::SetTouchSensor(on) if !v2 => {
                vec![op::TOUCH_SENSOR_SET, 0xD2, 0x01, u8::from(*on)]
            }
            Request::GetTouchSensor | Request::SetTouchSensor(_) => return None,
            Request::GetPauseWhenTakenOff => vec![
                op::SYSTEM_GET,
                if v2 { sys::V2_PAUSE } else { sys::V1_PAUSE },
            ],
            Request::SetPauseWhenTakenOff(on) => {
                if v2 {
                    vec![op::SYSTEM_SET, sys::V2_PAUSE, u8::from(!*on)]
                } else {
                    vec![op::SYSTEM_SET, sys::V1_PAUSE, 0x00, u8::from(*on)]
                }
            }
            Request::GetAutoPowerOff => {
                if v2 {
                    vec![op::V2_AUTO_POWER_OFF_GET, sys::V2_AUTO_POWER_OFF]
                } else {
                    vec![op::SYSTEM_GET, sys::V1_AUTO_POWER_OFF]
                }
            }
            Request::SetAutoPowerOff(apo) => {
                let [a, b] = apo.codes();
                if v2 {
                    vec![op::V2_AUTO_POWER_OFF_SET, sys::V2_AUTO_POWER_OFF, a, b]
                } else {
                    vec![op::SYSTEM_SET, sys::V1_AUTO_POWER_OFF, 0x01, a, b]
                }
            }
            Request::PowerOff => {
                if v2 {
                    vec![op::V2_POWER_SET, 0x03, 0x01]
                } else {
                    vec![op::V1_POWER_OFF, 0x00, 0x01]
                }
            }
        })
    }

    /// Decode a request (the inverse of [`Request::payload`]); used by the
    /// simulated headset.
    pub fn decode(p: &[u8], d: Dialect) -> Option<Request> {
        let v2 = d.is_v2();
        let at = |i: usize| p.get(i).copied();
        let opcode = at(0)?;
        let sub = at(1);
        Some(match opcode {
            op::INIT_REQUEST => Request::Init,
            op::FW_VERSION_GET => Request::GetFirmwareVersion,
            op::V1_BATTERY_GET if !v2 => Request::GetBattery(BatteryKind::from_code(sub?, d)?),
            op::V2_BATTERY_GET if v2 => Request::GetBattery(BatteryKind::from_code(sub?, d)?),
            op::V1_CODEC_GET if !v2 => Request::GetCodec,
            op::V2_CODEC_GET if v2 => Request::GetCodec,
            op::AMBIENT_GET => {
                if v2 && sub != Some(d.ambient.code()) {
                    return None;
                }
                Request::GetAmbientSound
            }
            op::AMBIENT_SET => {
                let value = parse_ambient(p, d).ok()?;
                let adjusting = !v2 && at(2) == Some(0x10);
                Request::SetAmbientSound { value, adjusting }
            }
            op::EQ_GET => Request::GetEqualizer,
            op::EQ_SET if at(3) == Some(0x00) => {
                Request::SetEqualizerPreset(EqPreset::from_code(at(2)?))
            }
            op::EQ_SET => Request::SetEqualizerBands(parse_eq(p)?),
            op::AUDIO_UPSAMPLING_GET => Request::GetAudioUpsampling,
            op::AUDIO_UPSAMPLING_SET => {
                Request::SetAudioUpsampling(at(if v2 { 2 } else { 3 })? == 1)
            }
            op::SYSTEM_GET if v2 => match sub? {
                sys::V2_STC => Request::GetSpeakToChat,
                sys::V2_PAUSE => Request::GetPauseWhenTakenOff,
                _ => return None,
            },
            op::SYSTEM_GET => match sub? {
                sys::V1_STC => Request::GetSpeakToChat,
                sys::V1_PAUSE => Request::GetPauseWhenTakenOff,
                sys::V1_AUTO_POWER_OFF => Request::GetAutoPowerOff,
                _ => return None,
            },
            op::SYSTEM_SET if v2 => match sub? {
                sys::V2_STC => Request::SetSpeakToChat(at(2)? == 0),
                sys::V2_PAUSE => Request::SetPauseWhenTakenOff(at(2)? == 0),
                _ => return None,
            },
            op::SYSTEM_SET => match sub? {
                sys::V1_STC => Request::SetSpeakToChat(at(3)? == 1),
                sys::V1_PAUSE => Request::SetPauseWhenTakenOff(at(3)? == 1),
                sys::V1_AUTO_POWER_OFF => {
                    Request::SetAutoPowerOff(AutoPowerOff::from_codes(at(3)?, at(4)?)?)
                }
                _ => return None,
            },
            op::STC_CONFIG_GET => Request::GetSpeakToChatConfig,
            op::STC_CONFIG_SET => Request::SetSpeakToChatConfig(parse_stc_config(p, d)?),
            op::TOUCH_SENSOR_GET if !v2 => Request::GetTouchSensor,
            op::TOUCH_SENSOR_SET if !v2 => Request::SetTouchSensor(at(3)? == 1),
            op::V2_AUTO_POWER_OFF_GET if v2 => Request::GetAutoPowerOff,
            op::V2_AUTO_POWER_OFF_SET if v2 => {
                Request::SetAutoPowerOff(AutoPowerOff::from_codes(at(2)?, at(3)?)?)
            }
            op::V1_POWER_OFF if !v2 => Request::PowerOff,
            op::V2_POWER_SET if v2 && sub == Some(0x03) => Request::PowerOff,
            _ => return None,
        })
    }
}

fn ambient_payload(opcode: u8, value: &AmbientSound, adjusting: bool, d: Dialect) -> Vec<u8> {
    let fov = u8::from(value.focus_on_voice);
    let level = value.level.min(AMBIENT_LEVEL_MAX);
    match d.version {
        ProtocolVersion::V1 => {
            let effect = match (value.mode, adjusting) {
                (AmbientMode::Off, _) => 0x00,
                (_, true) => 0x10,
                (_, false) => 0x11,
            };
            let nc = match value.mode {
                AmbientMode::NoiseCancelling => 0x02,
                AmbientMode::WindReduction => 0x01,
                AmbientMode::AmbientSound | AmbientMode::Off => 0x00,
            };
            let level = if value.mode == AmbientMode::AmbientSound {
                level.max(AMBIENT_LEVEL_MIN)
            } else {
                0
            };
            vec![opcode, 0x02, effect, 0x02, nc, 0x01, fov, level]
        }
        ProtocolVersion::V2 => {
            let on = u8::from(value.mode != AmbientMode::Off);
            let ambient = u8::from(value.mode == AmbientMode::AmbientSound);
            let mut p = vec![opcode, d.ambient.code(), 0x01, on, ambient];
            if d.ambient == AmbientType::WithWind {
                p.push(if value.mode == AmbientMode::WindReduction {
                    0x03
                } else {
                    0x02
                });
            }
            p.extend([fov, level]);
            p
        }
    }
}

fn parse_ambient(p: &[u8], d: Dialect) -> Result<AmbientSound, ParseError> {
    let opcode = p.first().copied().unwrap_or(0);
    match d.version {
        ProtocolVersion::V1 => {
            need(p, 8)?;
            let mode = if p[2] == 0x00 {
                AmbientMode::Off
            } else if p[3] == 0x02 {
                match p[4] {
                    0x02 => AmbientMode::NoiseCancelling,
                    0x01 => AmbientMode::WindReduction,
                    _ => AmbientMode::AmbientSound,
                }
            } else if p[4] == 0x01 {
                AmbientMode::NoiseCancelling
            } else {
                AmbientMode::AmbientSound
            };
            Ok(AmbientSound {
                mode,
                level: p[7].min(AMBIENT_LEVEL_MAX),
                focus_on_voice: p[6] == 0x01,
            })
        }
        ProtocolVersion::V2 => {
            need(p, 7)?;
            let kind = AmbientType::from_code(p[1]).ok_or(ParseError::Invalid {
                opcode,
                reason: "not an ambient sound message",
            })?;
            let with_wind = kind == AmbientType::WithWind && p.len() >= 8;
            let mode = if p[3] == 0x00 {
                AmbientMode::Off
            } else if with_wind && matches!(p[5], 0x03 | 0x05) {
                AmbientMode::WindReduction
            } else if p[4] == 0x01 {
                AmbientMode::AmbientSound
            } else {
                AmbientMode::NoiseCancelling
            };
            let i = if with_wind { 6 } else { 5 };
            Ok(AmbientSound {
                mode,
                level: p[i + 1].min(AMBIENT_LEVEL_MAX),
                focus_on_voice: p[i] == 0x01,
            })
        }
    }
}

fn eq_payload(opcode: u8, eq: &Equalizer) -> Vec<u8> {
    let mut p = vec![opcode, 0x00, eq.preset.code(), 0x06];
    p.extend(eq.bands.iter().map(|b| encode_band(*b)));
    p
}

fn parse_eq(p: &[u8]) -> Option<Equalizer> {
    let preset = EqPreset::from_code(*p.get(2)?);
    let mut bands = preset.default_bands();
    if let Some(&count) = p.get(3) {
        let count = usize::from(count).min(6);
        let values = p.get(4..4 + count)?;
        for (dst, src) in bands.iter_mut().zip(values) {
            *dst = decode_band(*src);
        }
    }
    Some(Equalizer { preset, bands })
}

fn parse_stc_config(p: &[u8], d: Dialect) -> Option<SpeakToChatConfig> {
    if d.is_v2() {
        Some(SpeakToChatConfig {
            sensitivity: StcSensitivity::from_code(*p.get(2)?)?,
            focus_on_voice: false,
            timeout: StcTimeout::from_code(*p.get(3)?)?,
        })
    } else {
        Some(SpeakToChatConfig {
            sensitivity: StcSensitivity::from_code(*p.get(3)?)?,
            focus_on_voice: *p.get(4)? == 1,
            timeout: StcTimeout::from_code(*p.get(5)?)?,
        })
    }
}

fn encode_band(v: i8) -> u8 {
    // Clamped to -10..=10, so the sum is always within 0..=20.
    u8::try_from(i16::from(v.clamp(EQ_MIN, EQ_MAX)) + 10).unwrap_or(10)
}

fn decode_band(v: u8) -> i8 {
    let v = i16::from(v) - 10;
    i8::try_from(v.clamp(i16::from(EQ_MIN), i16::from(EQ_MAX))).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Responses / notifications
// ---------------------------------------------------------------------------

/// Information reported by the headset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notification {
    InitReply(ProtocolVersion),
    FirmwareVersion(String),
    Battery {
        kind: BatteryKind,
        left: Battery,
        right: Option<Battery>,
    },
    Codec(Codec),
    AmbientSound(AmbientSound),
    Equalizer(Equalizer),
    AudioUpsampling(bool),
    SpeakToChat(bool),
    SpeakToChatConfig(SpeakToChatConfig),
    TouchSensor(bool),
    PauseWhenTakenOff(bool),
    AutoPowerOff(AutoPowerOff),
    /// Valid frame we do not interpret (raw payload).
    Unknown(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    Truncated {
        opcode: u8,
        needed: usize,
        got: usize,
    },
    Invalid {
        opcode: u8,
        reason: &'static str,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty payload"),
            ParseError::Truncated {
                opcode,
                needed,
                got,
            } => {
                write!(
                    f,
                    "message {opcode:#04x} truncated: needed {needed} bytes, got {got}"
                )
            }
            ParseError::Invalid { opcode, reason } => {
                write!(f, "message {opcode:#04x} invalid: {reason}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

fn need(p: &[u8], n: usize) -> Result<(), ParseError> {
    if p.len() < n {
        Err(ParseError::Truncated {
            opcode: p.first().copied().unwrap_or(0),
            needed: n,
            got: p.len(),
        })
    } else {
        Ok(())
    }
}

fn battery(level: u8, charging: u8) -> Battery {
    Battery {
        level: level.min(100),
        charging: charging == 0x01,
    }
}

impl Notification {
    pub fn parse(p: &[u8], d: Dialect) -> Result<Self, ParseError> {
        let &opcode = p.first().ok_or(ParseError::Empty)?;
        let invalid = |reason| ParseError::Invalid { opcode, reason };
        let v2 = d.is_v2();
        match opcode {
            op::INIT_REPLY => {
                // V1 headsets answer with 4 bytes, V2 headsets with 8.
                Ok(Notification::InitReply(if p.len() >= 8 {
                    ProtocolVersion::V2
                } else {
                    ProtocolVersion::V1
                }))
            }
            op::FW_VERSION_RET => {
                need(p, 3)?;
                let len = usize::from(p[2]);
                need(p, 3 + len)?;
                let text = String::from_utf8_lossy(&p[3..3 + len]).trim().to_string();
                Ok(Notification::FirmwareVersion(text))
            }
            op::V1_BATTERY_RET | op::V1_BATTERY_NOTIFY if !v2 => parse_battery(p, d),
            op::V2_BATTERY_RET | op::V2_BATTERY_NOTIFY if v2 => parse_battery(p, d),
            op::V1_CODEC_RET | op::V1_CODEC_NOTIFY if !v2 => {
                need(p, 3)?;
                Ok(Notification::Codec(Codec::from_code(p[2])))
            }
            op::V2_CODEC_RET | op::V2_CODEC_NOTIFY if v2 => {
                need(p, 3)?;
                Ok(Notification::Codec(Codec::from_code(p[2])))
            }
            op::AMBIENT_RET | op::AMBIENT_NOTIFY => {
                Ok(Notification::AmbientSound(parse_ambient(p, d)?))
            }
            op::EQ_RET | op::EQ_NOTIFY => {
                parse_eq(p)
                    .map(Notification::Equalizer)
                    .ok_or(ParseError::Truncated {
                        opcode,
                        needed: 10,
                        got: p.len(),
                    })
            }
            op::AUDIO_UPSAMPLING_RET | op::AUDIO_UPSAMPLING_NOTIFY => {
                let i = if v2 { 2 } else { 3 };
                need(p, i + 1)?;
                Ok(Notification::AudioUpsampling(p[i] == 0x01))
            }
            op::SYSTEM_RET | op::SYSTEM_NOTIFY => {
                need(p, 3)?;
                match (v2, p[1]) {
                    (true, sys::V2_STC) => Ok(Notification::SpeakToChat(p[2] == 0x00)),
                    (true, sys::V2_PAUSE) => Ok(Notification::PauseWhenTakenOff(p[2] == 0x00)),
                    (false, sys::V1_STC) => {
                        need(p, 4)?;
                        Ok(Notification::SpeakToChat(p[3] == 0x01))
                    }
                    (false, sys::V1_PAUSE) => {
                        need(p, 4)?;
                        Ok(Notification::PauseWhenTakenOff(p[3] == 0x01))
                    }
                    (false, sys::V1_AUTO_POWER_OFF) => {
                        need(p, 5)?;
                        AutoPowerOff::from_codes(p[3], p[4])
                            .map(Notification::AutoPowerOff)
                            .ok_or(invalid("unknown auto power off code"))
                    }
                    _ => Ok(Notification::Unknown(p.to_vec())),
                }
            }
            op::STC_CONFIG_RET | op::STC_CONFIG_NOTIFY => parse_stc_config(p, d)
                .map(Notification::SpeakToChatConfig)
                .ok_or(invalid("bad speak-to-chat config")),
            op::TOUCH_SENSOR_RET | op::TOUCH_SENSOR_NOTIFY if !v2 => {
                need(p, 4)?;
                Ok(Notification::TouchSensor(p[3] == 0x01))
            }
            op::V2_AUTO_POWER_OFF_RET | op::V2_AUTO_POWER_OFF_NOTIFY if v2 => {
                need(p, 4)?;
                if p[1] != sys::V2_AUTO_POWER_OFF {
                    return Ok(Notification::Unknown(p.to_vec()));
                }
                AutoPowerOff::from_codes(p[2], p[3])
                    .map(Notification::AutoPowerOff)
                    .ok_or(invalid("unknown auto power off code"))
            }
            _ => Ok(Notification::Unknown(p.to_vec())),
        }
    }

    /// Encode as the device would (used by the simulator and tests).
    pub fn payload(&self, d: Dialect) -> Vec<u8> {
        let v2 = d.is_v2();
        match self {
            Notification::InitReply(ProtocolVersion::V1) => vec![op::INIT_REPLY, 0x00, 0x40, 0x10],
            Notification::InitReply(ProtocolVersion::V2) => {
                vec![op::INIT_REPLY, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00]
            }
            Notification::FirmwareVersion(v) => {
                let bytes = v.as_bytes();
                let len = bytes.len().min(255);
                let mut p = vec![op::FW_VERSION_RET, 0x02, u8::try_from(len).unwrap_or(255)];
                p.extend_from_slice(&bytes[..len]);
                p
            }
            Notification::Battery { kind, left, right } => {
                let opcode = if v2 {
                    op::V2_BATTERY_NOTIFY
                } else {
                    op::V1_BATTERY_NOTIFY
                };
                let mut p = vec![opcode, kind.code(d), left.level, u8::from(left.charging)];
                if let Some(r) = right {
                    p.extend([r.level, u8::from(r.charging)]);
                }
                p
            }
            Notification::Codec(c) => {
                if v2 {
                    vec![op::V2_CODEC_NOTIFY, 0x02, c.code()]
                } else {
                    vec![op::V1_CODEC_NOTIFY, 0x00, c.code()]
                }
            }
            Notification::AmbientSound(a) => {
                let mut p = ambient_payload(op::AMBIENT_NOTIFY, a, false, d);
                if let Some(last) = p.last_mut() {
                    *last = a.level; // the headset reports the remembered level
                }
                p
            }
            Notification::Equalizer(eq) => eq_payload(op::EQ_NOTIFY, eq),
            Notification::AudioUpsampling(on) => {
                if v2 {
                    vec![op::AUDIO_UPSAMPLING_NOTIFY, 0x01, u8::from(*on)]
                } else {
                    vec![op::AUDIO_UPSAMPLING_NOTIFY, 0x02, 0x00, u8::from(*on)]
                }
            }
            Notification::SpeakToChat(on) => {
                if v2 {
                    vec![op::SYSTEM_NOTIFY, sys::V2_STC, u8::from(!*on), 0x01]
                } else {
                    vec![op::SYSTEM_NOTIFY, sys::V1_STC, 0x01, u8::from(*on)]
                }
            }
            Notification::SpeakToChatConfig(cfg) => {
                let mut p = Request::SetSpeakToChatConfig(*cfg)
                    .payload(d)
                    .unwrap_or_default();
                if let Some(first) = p.first_mut() {
                    *first = op::STC_CONFIG_NOTIFY;
                }
                p
            }
            Notification::TouchSensor(on) => {
                vec![op::TOUCH_SENSOR_NOTIFY, 0xD2, 0x01, u8::from(*on)]
            }
            Notification::PauseWhenTakenOff(on) => {
                if v2 {
                    vec![op::SYSTEM_NOTIFY, sys::V2_PAUSE, u8::from(!*on)]
                } else {
                    vec![op::SYSTEM_NOTIFY, sys::V1_PAUSE, 0x00, u8::from(*on)]
                }
            }
            Notification::AutoPowerOff(apo) => {
                let [a, b] = apo.codes();
                if v2 {
                    vec![op::V2_AUTO_POWER_OFF_NOTIFY, sys::V2_AUTO_POWER_OFF, a, b]
                } else {
                    vec![op::SYSTEM_NOTIFY, sys::V1_AUTO_POWER_OFF, 0x01, a, b]
                }
            }
            Notification::Unknown(p) => p.clone(),
        }
    }

    /// For V2 ambient-sound messages: which sub-type the headset uses.
    pub fn ambient_type_of(payload: &[u8]) -> Option<AmbientType> {
        match payload {
            [op::AMBIENT_RET | op::AMBIENT_NOTIFY, t, ..] => AmbientType::from_code(*t),
            _ => None,
        }
    }
}

fn parse_battery(p: &[u8], d: Dialect) -> Result<Notification, ParseError> {
    need(p, 4)?;
    let opcode = p[0];
    let kind = BatteryKind::from_code(p[1], d).ok_or(ParseError::Invalid {
        opcode,
        reason: "unknown battery type",
    })?;
    if kind.is_dual() {
        need(p, 6)?;
        Ok(Notification::Battery {
            kind,
            left: battery(p[2], p[3]),
            right: Some(battery(p[4], p[5])),
        })
    } else {
        Ok(Notification::Battery {
            kind,
            left: battery(p[2], p[3]),
            right: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XM5: Dialect = Dialect {
        version: ProtocolVersion::V2,
        ambient: AmbientType::Basic,
    };
    const WF_XM5: Dialect = Dialect {
        version: ProtocolVersion::V2,
        ambient: AmbientType::WithWind,
    };
    const ALL_DIALECTS: [Dialect; 3] = [Dialect::V1, XM5, WF_XM5];

    #[test]
    fn v1_ambient_bytes() {
        let nc = Request::SetAmbientSound {
            value: AmbientSound::default(),
            adjusting: false,
        };
        assert_eq!(
            nc.payload(Dialect::V1).unwrap(),
            vec![0x68, 0x02, 0x11, 0x02, 0x02, 0x01, 0x00, 0x00]
        );
        let asm = Request::SetAmbientSound {
            value: AmbientSound {
                mode: AmbientMode::AmbientSound,
                level: 15,
                focus_on_voice: true,
            },
            adjusting: true,
        };
        assert_eq!(
            asm.payload(Dialect::V1).unwrap(),
            vec![0x68, 0x02, 0x10, 0x02, 0x00, 0x01, 0x01, 15]
        );
    }

    #[test]
    fn v2_ambient_bytes_match_gadgetbridge() {
        let amb = AmbientSound {
            mode: AmbientMode::AmbientSound,
            level: 12,
            focus_on_voice: true,
        };
        let set = |v| Request::SetAmbientSound {
            value: v,
            adjusting: false,
        };
        // WH-1000XM5: 68 15 01 <on> <ambient> <voice> <level>
        assert_eq!(
            set(amb).payload(XM5).unwrap(),
            vec![0x68, 0x15, 0x01, 0x01, 0x01, 0x01, 12]
        );
        let nc = AmbientSound {
            mode: AmbientMode::NoiseCancelling,
            level: 12,
            focus_on_voice: false,
        };
        assert_eq!(
            set(nc).payload(XM5).unwrap(),
            vec![0x68, 0x15, 0x01, 0x01, 0x00, 0x00, 12]
        );
        let off = AmbientSound {
            mode: AmbientMode::Off,
            ..nc
        };
        assert_eq!(
            set(off).payload(XM5).unwrap(),
            vec![0x68, 0x15, 0x01, 0x00, 0x00, 0x00, 12]
        );
        // With wind byte (0x17)
        let wind = AmbientSound {
            mode: AmbientMode::WindReduction,
            ..nc
        };
        assert_eq!(
            set(wind).payload(WF_XM5).unwrap(),
            vec![0x68, 0x17, 0x01, 0x01, 0x00, 0x03, 0x00, 12]
        );
        assert_eq!(
            Request::GetAmbientSound.payload(XM5).unwrap(),
            vec![0x66, 0x15]
        );
    }

    #[test]
    fn v2_other_commands_match_gadgetbridge() {
        assert_eq!(
            Request::GetBattery(BatteryKind::Single)
                .payload(XM5)
                .unwrap(),
            vec![0x22, 0x00]
        );
        assert_eq!(Request::GetCodec.payload(XM5).unwrap(), vec![0x12, 0x02]);
        assert_eq!(
            Request::SetAudioUpsampling(true).payload(XM5).unwrap(),
            vec![0xE8, 0x01, 0x01]
        );
        assert_eq!(
            Request::SetSpeakToChat(true).payload(XM5).unwrap(),
            vec![0xF8, 0x0C, 0x00, 0x01]
        );
        assert_eq!(
            Request::SetPauseWhenTakenOff(true).payload(XM5).unwrap(),
            vec![0xF8, 0x01, 0x00]
        );
        assert_eq!(
            Request::SetAutoPowerOff(AutoPowerOff::WhenTakenOff)
                .payload(XM5)
                .unwrap(),
            vec![0x28, 0x05, 0x10, 0x00]
        );
        assert_eq!(
            Request::PowerOff.payload(XM5).unwrap(),
            vec![0x24, 0x03, 0x01]
        );
        assert_eq!(
            Request::SetEqualizerPreset(EqPreset::BassBoost)
                .payload(XM5)
                .unwrap(),
            vec![0x58, 0x00, 0x16, 0x00]
        );
        assert_eq!(Request::SetTouchSensor(true).payload(XM5), None);
    }

    #[test]
    fn v1_system_commands_match_gadgetbridge() {
        assert_eq!(
            Request::SetSpeakToChat(true).payload(Dialect::V1).unwrap(),
            vec![0xF8, 0x05, 0x01, 0x01]
        );
        assert_eq!(
            Request::SetPauseWhenTakenOff(false)
                .payload(Dialect::V1)
                .unwrap(),
            vec![0xF8, 0x03, 0x00, 0x00]
        );
        assert_eq!(
            Request::SetTouchSensor(true).payload(Dialect::V1).unwrap(),
            vec![0xD8, 0xD2, 0x01, 0x01]
        );
    }

    #[test]
    fn real_v2_replies_parse() {
        // Init replies captured by Gadgetbridge contributors.
        assert_eq!(
            Notification::parse(
                &[0x01, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00],
                Dialect::V1
            )
            .unwrap(),
            Notification::InitReply(ProtocolVersion::V2)
        );
        assert_eq!(
            Notification::parse(&[0x01, 0x00, 0x40, 0x10], Dialect::V1).unwrap(),
            Notification::InitReply(ProtocolVersion::V1)
        );
        // 0x13 means codec in V2 but battery in V1.
        assert_eq!(
            Notification::parse(&[0x13, 0x02, 0x10], XM5).unwrap(),
            Notification::Codec(Codec::Ldac)
        );
        assert_eq!(
            Notification::parse(&[0x23, 0x00, 55, 0x01], XM5).unwrap(),
            Notification::Battery {
                kind: BatteryKind::Single,
                left: Battery {
                    level: 55,
                    charging: true
                },
                right: None
            }
        );
    }

    #[test]
    fn requests_roundtrip_through_decode() {
        let samples = vec![
            Request::Init,
            Request::GetFirmwareVersion,
            Request::GetBattery(BatteryKind::Single),
            Request::GetCodec,
            Request::GetAmbientSound,
            Request::SetAmbientSound {
                value: AmbientSound {
                    mode: AmbientMode::AmbientSound,
                    level: 12,
                    focus_on_voice: true,
                },
                adjusting: false,
            },
            Request::SetAmbientSound {
                value: AmbientSound {
                    mode: AmbientMode::NoiseCancelling,
                    level: 0,
                    focus_on_voice: false,
                },
                adjusting: false,
            },
            Request::SetEqualizerPreset(EqPreset::Vocal),
            Request::SetEqualizerBands(Equalizer {
                preset: EqPreset::Manual,
                bands: [1, -2, 3, -4, 5, 10],
            }),
            Request::SetAudioUpsampling(true),
            Request::SetSpeakToChat(true),
            Request::SetSpeakToChat(false),
            Request::SetPauseWhenTakenOff(true),
            Request::SetPauseWhenTakenOff(false),
            Request::SetAutoPowerOff(AutoPowerOff::Off),
            Request::PowerOff,
        ];
        for d in ALL_DIALECTS {
            for r in &samples {
                let bytes = r.payload(d).unwrap();
                assert_eq!(
                    Request::decode(&bytes, d).as_ref(),
                    Some(r),
                    "{d:?} {bytes:02x?}"
                );
            }
        }
    }

    #[test]
    fn notifications_roundtrip() {
        let mut samples = vec![
            Notification::FirmwareVersion("2.0.1".into()),
            Notification::Codec(Codec::Aac),
            Notification::Equalizer(Equalizer {
                preset: EqPreset::Custom1,
                bands: [10, -10, 0, 3, -4, 7],
            }),
            Notification::AudioUpsampling(true),
            Notification::SpeakToChat(true),
            Notification::SpeakToChat(false),
            Notification::PauseWhenTakenOff(false),
            Notification::AutoPowerOff(AutoPowerOff::Off),
            Notification::Battery {
                kind: BatteryKind::Single,
                left: Battery {
                    level: 42,
                    charging: false,
                },
                right: None,
            },
            Notification::Battery {
                kind: BatteryKind::Dual,
                left: Battery {
                    level: 70,
                    charging: false,
                },
                right: Some(Battery {
                    level: 60,
                    charging: true,
                }),
            },
        ];
        for mode in [
            AmbientMode::NoiseCancelling,
            AmbientMode::AmbientSound,
            AmbientMode::Off,
        ] {
            samples.push(Notification::AmbientSound(AmbientSound {
                mode,
                level: 9,
                focus_on_voice: mode == AmbientMode::AmbientSound,
            }));
        }
        for d in ALL_DIALECTS {
            for n in &samples {
                let bytes = n.payload(d);
                assert_eq!(
                    &Notification::parse(&bytes, d).unwrap(),
                    n,
                    "{d:?} {bytes:02x?}"
                );
            }
        }
        let wind = Notification::AmbientSound(AmbientSound {
            mode: AmbientMode::WindReduction,
            level: 3,
            focus_on_voice: false,
        });
        assert_eq!(
            Notification::parse(&wind.payload(WF_XM5), WF_XM5).unwrap(),
            wind
        );
        assert_eq!(
            Notification::parse(&wind.payload(Dialect::V1), Dialect::V1).unwrap(),
            wind
        );
    }

    #[test]
    fn equalizer_clamping() {
        let wild = Request::SetEqualizerBands(Equalizer {
            preset: EqPreset::Manual,
            bands: [127, -128, 0, 0, 0, 0],
        });
        assert_eq!(&wild.payload(XM5).unwrap()[4..6], &[20, 0]);
    }

    #[test]
    fn malformed_input_never_panics() {
        for d in ALL_DIALECTS {
            for opcode in 0..=255u8 {
                for len in 0..10 {
                    let mut p = vec![opcode];
                    p.extend(std::iter::repeat_n(0x09, len));
                    let _ = Notification::parse(&p, d);
                    let _ = Request::decode(&p, d);
                }
            }
        }
        assert_eq!(Notification::parse(&[], XM5), Err(ParseError::Empty));
    }

    #[test]
    fn battery_levels_are_clamped() {
        assert!(matches!(
            Notification::parse(&[0x11, 0x00, 250, 0x00], Dialect::V1).unwrap(),
            Notification::Battery {
                left: Battery { level: 100, .. },
                ..
            }
        ));
    }

    #[test]
    fn eq_preset_codes_are_unique() {
        for p in EqPreset::ALL {
            assert_eq!(EqPreset::from_code(p.code()), p);
        }
    }
}
