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

use super::frame::DataType;

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
    /// `0x19`: adds "Auto Ambient Sound" (noise adaptation), e.g. WH-1000XM6.
    NoiseAdaptive,
}

impl AmbientType {
    fn code(self) -> u8 {
        match self {
            AmbientType::Basic => 0x15,
            AmbientType::WithWind => 0x17,
            AmbientType::NoiseAdaptive => 0x19,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            0x15 => Some(AmbientType::Basic),
            0x17 => Some(AmbientType::WithWind),
            0x19 => Some(AmbientType::NoiseAdaptive),
            _ => None,
        }
    }

    pub const ALL: [AmbientType; 3] = [
        AmbientType::Basic,
        AmbientType::WithWind,
        AmbientType::NoiseAdaptive,
    ];
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

    /// V1 only: virtual surround (VPT) / sound position.
    pub const V1_SURROUND_GET: u8 = 0x46;
    pub const V1_SURROUND_RET: u8 = 0x47;
    pub const V1_SURROUND_SET: u8 = 0x48;
    pub const V1_SURROUND_NOTIFY: u8 = 0x49;
    /// V2: is the equalizer currently usable (BGM/Cinema can block it).
    pub const V2_EQ_STATUS_GET: u8 = 0x52;
    pub const V2_EQ_STATUS_RET: u8 = 0x53;
    pub const V2_EQ_STATUS_NOTIFY: u8 = 0x55;
    pub const EQ_GET: u8 = 0x56;
    pub const EQ_RET: u8 = 0x57;
    pub const EQ_SET: u8 = 0x58;
    pub const EQ_NOTIFY: u8 = 0x59;
    pub const AMBIENT_GET: u8 = 0x66;
    pub const AMBIENT_RET: u8 = 0x67;
    pub const AMBIENT_SET: u8 = 0x68;
    pub const AMBIENT_NOTIFY: u8 = 0x69;
    /// V2 playback control (sub-type 0x01) and music volume (sub-type 0x20).
    pub const V2_PLAY_STATUS_GET: u8 = 0xA2;
    pub const V2_PLAY_STATUS_RET: u8 = 0xA3;
    pub const V2_PLAY_STATUS_SET: u8 = 0xA4;
    pub const V2_PLAY_STATUS_NOTIFY: u8 = 0xA5;
    pub const V2_PLAY_PARAM_GET: u8 = 0xA6;
    pub const V2_PLAY_PARAM_RET: u8 = 0xA7;
    pub const V2_PLAY_PARAM_SET: u8 = 0xA8;
    pub const V2_PLAY_PARAM_NOTIFY: u8 = 0xA9;
    /// V2 "general settings" (slots 0xD1..=0xD4, e.g. touch panel, multipoint).
    /// GET/SET/RET/NTFY param share the V1 touch-sensor opcodes 0xD6..0xD9.
    pub const V2_GS_GET_CAPABILITY: u8 = 0xD0;
    pub const V2_GS_RET_CAPABILITY: u8 = 0xD1;
    pub const TOUCH_SENSOR_GET: u8 = 0xD6;
    pub const TOUCH_SENSOR_RET: u8 = 0xD7;
    pub const TOUCH_SENSOR_SET: u8 = 0xD8;
    pub const TOUCH_SENSOR_NOTIFY: u8 = 0xD9;
    /// "Audio" family. Sub-types (V2): 0x01 upscaling (DSEE),
    /// 0x03 BGM listening mode, 0x04 Cinema listening mode.
    /// V2 audio *status* (is a feature usable right now); sub-type 0x01 = DSEE.
    pub const V2_AUDIO_STATUS_RET: u8 = 0xE3;
    pub const V2_AUDIO_STATUS_NOTIFY: u8 = 0xE5;
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

/// Opcodes of the secondary channel ("table 2", frame data type 0x0E).
pub mod op2 {
    pub const VOICE_GUIDANCE_GET: u8 = 0x46;
    pub const VOICE_GUIDANCE_RET: u8 = 0x47;
    pub const VOICE_GUIDANCE_SET: u8 = 0x48;
    pub const VOICE_GUIDANCE_NOTIFY: u8 = 0x49;
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
    /// "Auto Ambient Sound" (only on [`AmbientType::NoiseAdaptive`] headsets).
    pub auto_ambient: bool,
    pub auto_sensitivity: AutoAmbientSensitivity,
}

/// Sensitivity of "Auto Ambient Sound".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AutoAmbientSensitivity {
    #[default]
    Standard,
    High,
    Low,
}

impl AutoAmbientSensitivity {
    pub const ALL: [AutoAmbientSensitivity; 3] = [
        AutoAmbientSensitivity::Standard,
        AutoAmbientSensitivity::High,
        AutoAmbientSensitivity::Low,
    ];
    fn code(self) -> u8 {
        match self {
            AutoAmbientSensitivity::Standard => 0,
            AutoAmbientSensitivity::High => 1,
            AutoAmbientSensitivity::Low => 2,
        }
    }
    fn from_code(c: u8) -> Self {
        AutoAmbientSensitivity::ALL
            .into_iter()
            .find(|x| x.code() == c)
            .unwrap_or_default()
    }
    pub fn label(self) -> &'static str {
        match self {
            AutoAmbientSensitivity::Standard => "Standard",
            AutoAmbientSensitivity::High => "High",
            AutoAmbientSensitivity::Low => "Low",
        }
    }
}

impl Default for AmbientSound {
    fn default() -> Self {
        Self {
            mode: AmbientMode::NoiseCancelling,
            level: 20,
            focus_on_voice: false,
            auto_ambient: false,
            auto_sensitivity: AutoAmbientSensitivity::Standard,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EqPreset {
    #[default]
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
    /// Presets of 10-band headsets (WH-1000XM6).
    Heavy,
    Clear,
    Hard,
    Soft,
    Unknown(u8),
}

impl EqPreset {
    pub const ALL: [EqPreset; 16] = [
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
        EqPreset::Heavy,
        EqPreset::Clear,
        EqPreset::Hard,
        EqPreset::Soft,
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
            EqPreset::Heavy => 0x30,
            EqPreset::Clear => 0x31,
            EqPreset::Hard => 0x32,
            EqPreset::Soft => 0x33,
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
            EqPreset::Heavy => "Heavy",
            EqPreset::Clear => "Clear",
            EqPreset::Hard => "Hard",
            EqPreset::Soft => "Soft",
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
        // Curves of the Clear Bass + 5 band presets; 10-band headsets report theirs.
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

/// Band layout of the equalizer, decided by what the headset reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EqLayout {
    /// Clear Bass + 400 Hz, 1k, 2.5k, 6.3k, 16k; values -10..=10 (WH-1000XM5 and older).
    #[default]
    ClearBassFiveBand,
    /// 31 Hz .. 16 kHz; values -6..=6 (WH-1000XM6).
    TenBand,
}

impl EqLayout {
    pub fn band_count(self) -> usize {
        match self {
            EqLayout::ClearBassFiveBand => 6,
            EqLayout::TenBand => 10,
        }
    }

    fn from_count(count: u8) -> Option<Self> {
        match count {
            6 => Some(EqLayout::ClearBassFiveBand),
            10 => Some(EqLayout::TenBand),
            _ => None,
        }
    }

    /// Wire value of a band set to 0 dB.
    fn offset(self) -> i16 {
        match self {
            EqLayout::ClearBassFiveBand => 10,
            EqLayout::TenBand => 6,
        }
    }

    pub fn range(self) -> (i8, i8) {
        match self {
            EqLayout::ClearBassFiveBand => (-10, 10),
            EqLayout::TenBand => (-6, 6),
        }
    }

    pub fn labels(self) -> &'static [&'static str] {
        match self {
            EqLayout::ClearBassFiveBand => &EQ_BAND_LABELS,
            EqLayout::TenBand => &[
                "31", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k",
            ],
        }
    }

    /// Presets offered for this layout.
    pub fn presets(self) -> &'static [EqPreset] {
        match self {
            EqLayout::ClearBassFiveBand => &EqPreset::ALL[..12],
            EqLayout::TenBand => &[
                EqPreset::Off,
                EqPreset::Heavy,
                EqPreset::Clear,
                EqPreset::Hard,
                EqPreset::Soft,
                EqPreset::Manual,
                EqPreset::Custom1,
                EqPreset::Custom2,
            ],
        }
    }

    fn encode(self, v: i8) -> u8 {
        let (lo, hi) = self.range();
        // Clamped, so the result is always within 0..=20.
        u8::try_from(i16::from(v.clamp(lo, hi)) + self.offset()).unwrap_or(0)
    }

    fn decode(self, v: u8) -> i8 {
        let (lo, hi) = self.range();
        let v = (i16::from(v) - self.offset()).clamp(i16::from(lo), i16::from(hi));
        i8::try_from(v).unwrap_or(0)
    }
}

/// Equalizer: preset + band values (see [`EqLayout`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Equalizer {
    pub preset: EqPreset,
    pub layout: EqLayout,
    /// Only the first `layout.band_count()` values are used.
    pub bands: [i8; 10],
}

impl Equalizer {
    /// A Clear Bass + 5 band equalizer (`[clear_bass, 400, 1k, 2.5k, 6.3k, 16k]`).
    pub fn classic(preset: EqPreset, values: [i8; 6]) -> Self {
        let mut bands = [0; 10];
        bands[..6].copy_from_slice(&values);
        Self {
            preset,
            layout: EqLayout::ClearBassFiveBand,
            bands,
        }
    }

    pub fn ten_band(preset: EqPreset, bands: [i8; 10]) -> Self {
        Self {
            preset,
            layout: EqLayout::TenBand,
            bands,
        }
    }

    pub fn values(&self) -> &[i8] {
        &self.bands[..self.layout.band_count()]
    }

    pub fn values_mut(&mut self) -> &mut [i8] {
        let n = self.layout.band_count();
        &mut self.bands[..n]
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

/// Room size of the BGM listening mode (labels as in the phone app).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RoomSize {
    #[default]
    Small,
    Middle,
    Large,
}

impl RoomSize {
    pub const ALL: [RoomSize; 3] = [RoomSize::Small, RoomSize::Middle, RoomSize::Large];
    fn code(self) -> u8 {
        match self {
            RoomSize::Small => 0x00,
            RoomSize::Middle => 0x01,
            RoomSize::Large => 0x02,
        }
    }
    fn from_code(c: u8) -> Option<Self> {
        RoomSize::ALL.into_iter().find(|x| x.code() == c)
    }
    pub fn label(self) -> &'static str {
        match self {
            RoomSize::Small => "My Room",
            RoomSize::Middle => "Living Room",
            RoomSize::Large => "Café",
        }
    }
}

/// Listening mode of newer headsets (WH-1000XM6, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ListeningMode {
    #[default]
    Standard,
    /// Background music: sound as if played from speakers in a room.
    Bgm,
    /// Up-mixed, cinema-like spatial sound.
    Cinema,
}

/// Virtual surround presets of older (V1) headsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Surround {
    #[default]
    Off,
    OutdoorStage,
    Arena,
    ConcertHall,
    Club,
}

impl Surround {
    pub const ALL: [Surround; 5] = [
        Surround::Off,
        Surround::Arena,
        Surround::Club,
        Surround::OutdoorStage,
        Surround::ConcertHall,
    ];
    fn code(self) -> u8 {
        match self {
            Surround::Off => 0x00,
            Surround::OutdoorStage => 0x01,
            Surround::Arena => 0x02,
            Surround::ConcertHall => 0x03,
            Surround::Club => 0x04,
        }
    }
    fn from_code(c: u8) -> Option<Self> {
        Surround::ALL.into_iter().find(|x| x.code() == c)
    }
    pub fn label(self) -> &'static str {
        match self {
            Surround::Off => "Off",
            Surround::OutdoorStage => "Outdoor Stage",
            Surround::Arena => "Arena",
            Surround::ConcertHall => "Concert Hall",
            Surround::Club => "Club",
        }
    }
}

/// Bluetooth connection quality preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ConnectionPriority {
    #[default]
    SoundQuality,
    StableConnection,
    LowLatency,
}

impl ConnectionPriority {
    pub const CHOICES: [ConnectionPriority; 2] = [
        ConnectionPriority::SoundQuality,
        ConnectionPriority::StableConnection,
    ];
    fn code(self) -> u8 {
        match self {
            ConnectionPriority::SoundQuality => 0x00,
            ConnectionPriority::StableConnection => 0x01,
            ConnectionPriority::LowLatency => 0x02,
        }
    }
    fn from_code(c: u8) -> Option<Self> {
        [Self::SoundQuality, Self::StableConnection, Self::LowLatency]
            .into_iter()
            .find(|x| x.code() == c)
    }
    pub fn label(self) -> &'static str {
        match self {
            ConnectionPriority::SoundQuality => "Priority on Sound Quality",
            ConnectionPriority::StableConnection => "Priority on Stable Connection",
            ConnectionPriority::LowLatency => "Priority on Low Latency",
        }
    }
}

/// Playback commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackControl {
    Play,
    Pause,
    Next,
    Previous,
}

impl PlaybackControl {
    fn code(self) -> u8 {
        match self {
            PlaybackControl::Pause => 0x01,
            PlaybackControl::Next => 0x02,
            PlaybackControl::Previous => 0x03,
            PlaybackControl::Play => 0x07,
        }
    }
    fn from_code(c: u8) -> Option<Self> {
        [Self::Play, Self::Pause, Self::Next, Self::Previous]
            .into_iter()
            .find(|x| x.code() == c)
    }
}

/// Playback state reported by the headset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Unknown,
    Playing,
    Paused,
    Stopped,
}

impl PlaybackState {
    fn code(self) -> u8 {
        match self {
            PlaybackState::Unknown => 0x00,
            PlaybackState::Playing => 0x01,
            PlaybackState::Paused => 0x02,
            PlaybackState::Stopped => 0x03,
        }
    }
    fn from_code(c: u8) -> Self {
        match c {
            0x01 => PlaybackState::Playing,
            0x02 => PlaybackState::Paused,
            0x03 => PlaybackState::Stopped,
            _ => PlaybackState::Unknown,
        }
    }
}

/// Music volume range used by the headset (0..=30).
pub const VOLUME_MAX: u8 = 30;
/// Voice guidance volume range (-2..=2).
pub const VOICE_VOLUME_RANGE: (i8, i8) = (-2, 2);

/// What the NC/AMB button on over-ear headphones cycles through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NcAmbButton {
    #[default]
    NcAmbientOff,
    NcAmbient,
    NcOff,
    AmbientOff,
}

impl NcAmbButton {
    pub const ALL: [NcAmbButton; 4] = [
        NcAmbButton::NcAmbientOff,
        NcAmbButton::NcAmbient,
        NcAmbButton::NcOff,
        NcAmbButton::AmbientOff,
    ];
    fn code(self) -> u8 {
        match self {
            NcAmbButton::NcAmbientOff => 0x01,
            NcAmbButton::NcAmbient => 0x02,
            NcAmbButton::NcOff => 0x03,
            NcAmbButton::AmbientOff => 0x04,
        }
    }
    fn from_code(c: u8) -> Option<Self> {
        NcAmbButton::ALL.into_iter().find(|x| x.code() == c)
    }
    pub fn label(self) -> &'static str {
        match self {
            NcAmbButton::NcAmbientOff => "Noise Canceling / Ambient Sound / Off",
            NcAmbButton::NcAmbient => "Noise Canceling / Ambient Sound",
            NcAmbButton::NcOff => "Noise Canceling / Off",
            NcAmbButton::AmbientOff => "Ambient Sound / Off",
        }
    }
}

/// Longest name/summary of a general setting we keep.
const GS_TEXT_MAX: usize = 200;

/// General-setting slot 1..=4 <-> wire code 0xD1..=0xD4.
fn gs_slot_code(slot: u8) -> Option<u8> {
    (1..=4).contains(&slot).then(|| 0xD0 + slot)
}

fn gs_slot(code: u8) -> Option<u8> {
    (0xD1..=0xD4).contains(&code).then(|| code - 0xD0)
}

/// Friendly label for the enum-style names some headsets report.
fn gs_label(raw: &str) -> String {
    match raw {
        "TOUCH_PANEL_SETTING" => "Touch panel".into(),
        "MULTIPOINT_SETTING" => "Connect to 2 devices simultaneously".into(),
        "CAPTURE_VOICE_DURING_A_PHONE_CALL_SETTING" => "Capture voice during a phone call".into(),
        _ if raw.contains('_') && raw == raw.to_uppercase() => {
            // e.g. SOME_FEATURE_SETTING -> "Some feature"
            let words: Vec<&str> = raw
                .split('_')
                .filter(|w| !w.is_empty() && *w != "SETTING")
                .collect();
            let mut text = words.join(" ").to_lowercase();
            if let Some(first) = text.get(..1) {
                text = first.to_uppercase() + text.get(1..).unwrap_or("");
            }
            text
        }
        _ => raw.to_string(),
    }
}

/// `[D1, slot, setting type, string format, len, subject.., len, summary..]`
fn parse_gs_info(p: &[u8]) -> Result<Notification, ParseError> {
    let opcode = p.first().copied().unwrap_or(0);
    let invalid = |reason| ParseError::Invalid { opcode, reason };
    need(p, 6)?;
    let slot = gs_slot(p[1]).ok_or(invalid("unknown general setting"))?;
    if p[2] != 0x00 {
        return Ok(Notification::Unknown(p.to_vec())); // list-type: not supported
    }
    let mut rest = &p[4..];
    let mut texts = Vec::with_capacity(2);
    for _ in 0..2 {
        let (&len, tail) = rest.split_first().ok_or(invalid("missing text length"))?;
        let len = usize::from(len);
        let bytes = tail.get(..len).ok_or(invalid("text longer than message"))?;
        let text: String = String::from_utf8_lossy(&bytes[..len.min(GS_TEXT_MAX)])
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        texts.push(text);
        rest = &tail[len..];
    }
    let summary = texts.pop().unwrap_or_default();
    let raw = texts.pop().unwrap_or_default();
    let name = if p[3] == 0x01 { gs_label(&raw) } else { raw };
    Ok(Notification::GeneralSettingInfo {
        slot,
        name,
        summary,
    })
}

/// V2 "enable/disable" byte: 0x00 means enabled, 0x01 disabled.
fn v2_flag(on: bool) -> u8 {
    u8::from(!on)
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
    /// V2 listening mode: query the BGM part.
    GetBgmMode,
    /// V2 listening mode: turn BGM on/off (off returns to Standard).
    SetBgmMode {
        on: bool,
        room: RoomSize,
    },
    /// V2 listening mode: query the Cinema part.
    GetCinemaMode,
    /// V2 listening mode: turn Cinema on/off (off returns to Standard).
    SetCinemaMode(bool),
    /// V1 virtual surround.
    GetSurround,
    SetSurround(Surround),
    /// V2: whether the equalizer can currently be used.
    GetEqAvailability,
    GetConnectionPriority,
    /// Changing this briefly reconnects the audio stream.
    SetConnectionPriority(ConnectionPriority),
    GetNcAmbButton,
    SetNcAmbButton(NcAmbButton),
    GetPlayback,
    Playback(PlaybackControl),
    GetVolume,
    SetVolume(u8),
    /// Secondary channel (frame type 0x0E).
    GetVoiceGuidance,
    SetVoiceGuidance(bool),
    GetVoiceGuidanceVolume,
    SetVoiceGuidanceVolume(i8),
    /// V2 general setting `slot` (1..=4): ask for its name and type.
    GetGeneralSettingInfo(u8),
    GetGeneralSetting(u8),
    SetGeneralSetting {
        slot: u8,
        on: bool,
    },
    PowerOff,
}

impl Request {
    fn is_v2_only(&self) -> bool {
        matches!(
            self,
            Request::GetEqAvailability
                | Request::GetConnectionPriority
                | Request::SetConnectionPriority(_)
                | Request::GetNcAmbButton
                | Request::SetNcAmbButton(_)
                | Request::GetPlayback
                | Request::Playback(_)
                | Request::GetVolume
                | Request::SetVolume(_)
                | Request::GetVoiceGuidance
                | Request::SetVoiceGuidance(_)
                | Request::GetVoiceGuidanceVolume
                | Request::SetVoiceGuidanceVolume(_)
                | Request::GetGeneralSettingInfo(_)
                | Request::GetGeneralSetting(_)
                | Request::SetGeneralSetting { .. }
        )
    }

    /// Frame type: most requests use the main channel; voice guidance uses
    /// the secondary one.
    pub fn data_type(&self) -> DataType {
        match self {
            Request::GetVoiceGuidance
            | Request::SetVoiceGuidance(_)
            | Request::GetVoiceGuidanceVolume
            | Request::SetVoiceGuidanceVolume(_) => DataType::DataMdrNo2,
            _ => DataType::DataMdr,
        }
    }

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
            Request::GetBgmMode if v2 => vec![op::AUDIO_UPSAMPLING_GET, 0x03],
            Request::SetBgmMode { on, room } if v2 => {
                vec![op::AUDIO_UPSAMPLING_SET, 0x03, v2_flag(*on), room.code()]
            }
            Request::GetCinemaMode if v2 => vec![op::AUDIO_UPSAMPLING_GET, 0x04],
            Request::SetCinemaMode(on) if v2 => vec![op::AUDIO_UPSAMPLING_SET, 0x04, v2_flag(*on)],
            Request::GetBgmMode
            | Request::SetBgmMode { .. }
            | Request::GetCinemaMode
            | Request::SetCinemaMode(_) => return None,
            Request::GetSurround if !v2 => vec![op::V1_SURROUND_GET, 0x01],
            Request::SetSurround(mode) if !v2 => vec![op::V1_SURROUND_SET, 0x01, mode.code()],
            Request::GetSurround | Request::SetSurround(_) => return None,
            _ if !v2 && self.is_v2_only() => return None,
            Request::GetEqAvailability => vec![op::V2_EQ_STATUS_GET, 0x00],
            Request::GetConnectionPriority => vec![op::AUDIO_UPSAMPLING_GET, 0x00],
            Request::SetConnectionPriority(prio) => {
                vec![op::AUDIO_UPSAMPLING_SET, 0x00, prio.code()]
            }
            Request::GetNcAmbButton => vec![op::AMBIENT_GET, 0x30],
            Request::SetNcAmbButton(b) => vec![op::AMBIENT_SET, 0x30, b.code()],
            Request::GetPlayback => vec![op::V2_PLAY_STATUS_GET, 0x01],
            // Status byte 0x00 = "enable".
            Request::Playback(c) => vec![op::V2_PLAY_STATUS_SET, 0x01, 0x00, c.code()],
            Request::GetVolume => vec![op::V2_PLAY_PARAM_GET, 0x20],
            Request::SetVolume(v) => vec![op::V2_PLAY_PARAM_SET, 0x20, (*v).min(VOLUME_MAX)],
            // 0x01 = ask for English names.
            Request::GetGeneralSettingInfo(slot) => {
                vec![op::V2_GS_GET_CAPABILITY, gs_slot_code(*slot)?, 0x01]
            }
            Request::GetGeneralSetting(slot) => vec![op::TOUCH_SENSOR_GET, gs_slot_code(*slot)?],
            // 0x00 = boolean setting; value 0x00 = on.
            Request::SetGeneralSetting { slot, on } => {
                vec![
                    op::TOUCH_SENSOR_SET,
                    gs_slot_code(*slot)?,
                    0x00,
                    v2_flag(*on),
                ]
            }
            Request::GetVoiceGuidance => vec![op2::VOICE_GUIDANCE_GET, 0x01],
            Request::SetVoiceGuidance(on) => vec![op2::VOICE_GUIDANCE_SET, 0x01, v2_flag(*on)],
            Request::GetVoiceGuidanceVolume => vec![op2::VOICE_GUIDANCE_GET, 0x20],
            Request::SetVoiceGuidanceVolume(v) => {
                let (lo, hi) = VOICE_VOLUME_RANGE;
                // Signed byte; the trailing 0x00 asks for a feedback beep.
                vec![
                    op2::VOICE_GUIDANCE_SET,
                    0x20,
                    (*v).clamp(lo, hi).to_be_bytes()[0],
                    0x00,
                ]
            }
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

    /// Decode a secondary-channel (frame type 0x0E) request.
    pub fn decode_t2(p: &[u8], d: Dialect) -> Option<Request> {
        if !d.is_v2() {
            return None;
        }
        Some(match (*p.first()?, *p.get(1)?) {
            (op2::VOICE_GUIDANCE_GET, 0x01) => Request::GetVoiceGuidance,
            (op2::VOICE_GUIDANCE_GET, 0x20) => Request::GetVoiceGuidanceVolume,
            (op2::VOICE_GUIDANCE_SET, 0x01) => Request::SetVoiceGuidance(*p.get(2)? == 0x00),
            (op2::VOICE_GUIDANCE_SET, 0x20) => {
                Request::SetVoiceGuidanceVolume(i8::from_be_bytes([*p.get(2)?]))
            }
            _ => return None,
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
            op::AMBIENT_GET if v2 && sub == Some(0x30) => Request::GetNcAmbButton,
            op::AMBIENT_SET if v2 && sub == Some(0x30) => {
                Request::SetNcAmbButton(NcAmbButton::from_code(at(2)?)?)
            }
            op::AMBIENT_GET => {
                if v2 && sub != Some(d.ambient.code()) {
                    return None;
                }
                Request::GetAmbientSound
            }
            op::AMBIENT_SET => {
                let value = parse_ambient(p, d).ok()?;
                let adjusting = if v2 {
                    at(2) == Some(0x00)
                } else {
                    at(2) == Some(0x10)
                };
                Request::SetAmbientSound { value, adjusting }
            }
            op::EQ_GET => Request::GetEqualizer,
            op::EQ_SET if at(3) == Some(0x00) => {
                Request::SetEqualizerPreset(EqPreset::from_code(at(2)?))
            }
            op::EQ_SET => Request::SetEqualizerBands(parse_eq(p)?),
            op::AUDIO_UPSAMPLING_GET if v2 => match sub? {
                0x00 => Request::GetConnectionPriority,
                0x01 => Request::GetAudioUpsampling,
                0x03 => Request::GetBgmMode,
                0x04 => Request::GetCinemaMode,
                _ => return None,
            },
            op::AUDIO_UPSAMPLING_SET if v2 => match sub? {
                0x00 => Request::SetConnectionPriority(ConnectionPriority::from_code(at(2)?)?),
                0x01 => Request::SetAudioUpsampling(at(2)? == 1),
                0x03 => Request::SetBgmMode {
                    on: at(2)? == 0,
                    room: RoomSize::from_code(at(3)?)?,
                },
                0x04 => Request::SetCinemaMode(at(2)? == 0),
                _ => return None,
            },
            op::AUDIO_UPSAMPLING_GET => Request::GetAudioUpsampling,
            op::AUDIO_UPSAMPLING_SET => Request::SetAudioUpsampling(at(3)? == 1),
            op::V1_SURROUND_GET if !v2 && sub == Some(0x01) => Request::GetSurround,
            op::V1_SURROUND_SET if !v2 && sub == Some(0x01) => {
                Request::SetSurround(Surround::from_code(at(2)?)?)
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
            op::V2_EQ_STATUS_GET if v2 => Request::GetEqAvailability,
            op::V2_GS_GET_CAPABILITY if v2 => Request::GetGeneralSettingInfo(gs_slot(sub?)?),
            op::TOUCH_SENSOR_GET if v2 => Request::GetGeneralSetting(gs_slot(sub?)?),
            op::TOUCH_SENSOR_SET if v2 && at(2) == Some(0x00) => Request::SetGeneralSetting {
                slot: gs_slot(sub?)?,
                on: at(3)? == 0x00,
            },
            op::V2_PLAY_STATUS_GET if v2 && sub == Some(0x01) => Request::GetPlayback,
            op::V2_PLAY_STATUS_SET if v2 && sub == Some(0x01) => {
                Request::Playback(PlaybackControl::from_code(at(3)?)?)
            }
            op::V2_PLAY_PARAM_GET if v2 && sub == Some(0x20) => Request::GetVolume,
            op::V2_PLAY_PARAM_SET if v2 && sub == Some(0x20) => Request::SetVolume(at(2)?),
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
            // Value-change status: 0x00 while a slider is dragged, 0x01 when done.
            let status = u8::from(!adjusting);
            let mut p = vec![opcode, d.ambient.code(), status, on, ambient];
            if d.ambient == AmbientType::NoiseAdaptive {
                p.extend([
                    fov,
                    level.max(AMBIENT_LEVEL_MIN),
                    u8::from(value.auto_ambient),
                    value.auto_sensitivity.code(),
                ]);
                return p;
            }
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
                ..AmbientSound::default()
            })
        }
        ProtocolVersion::V2 => {
            need(p, 7)?;
            let kind = AmbientType::from_code(p[1]).ok_or(ParseError::Invalid {
                opcode,
                reason: "not an ambient sound message",
            })?;
            if kind == AmbientType::NoiseAdaptive {
                // [op, 0x19, status, on, nc/asm, voice, level, auto on, auto sensitivity]
                need(p, 9)?;
                let mode = if p[3] == 0x00 {
                    AmbientMode::Off
                } else if p[4] == 0x01 {
                    AmbientMode::AmbientSound
                } else {
                    AmbientMode::NoiseCancelling
                };
                return Ok(AmbientSound {
                    mode,
                    level: p[6].min(AMBIENT_LEVEL_MAX),
                    focus_on_voice: p[5] == 0x01,
                    auto_ambient: p[7] == 0x01,
                    auto_sensitivity: AutoAmbientSensitivity::from_code(p[8]),
                });
            }
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
                ..AmbientSound::default()
            })
        }
    }
}

fn eq_payload(opcode: u8, eq: &Equalizer) -> Vec<u8> {
    let count = u8::try_from(eq.layout.band_count()).unwrap_or(6);
    let mut p = vec![opcode, 0x00, eq.preset.code(), count];
    p.extend(eq.values().iter().map(|b| eq.layout.encode(*b)));
    p
}

/// `[op, 0x00, preset, n, bands...]`; `n` is 6 (Clear Bass + 5) or 10, and 0
/// when only a preset is reported.
fn parse_eq(p: &[u8]) -> Option<Equalizer> {
    let preset = EqPreset::from_code(*p.get(2)?);
    let count = p.get(3).copied().unwrap_or(0);
    let Some(layout) = EqLayout::from_count(count) else {
        return Some(Equalizer::classic(preset, preset.default_bands()));
    };
    let values = p.get(4..4 + layout.band_count())?;
    let mut eq = Equalizer {
        preset,
        layout,
        bands: [0; 10],
    };
    for (dst, src) in eq.values_mut().iter_mut().zip(values) {
        *dst = layout.decode(*src);
    }
    Some(eq)
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
    /// V2: whether DSEE can be used right now (BGM/Cinema can block it).
    UpsamplingAvailable(bool),
    BgmMode {
        on: bool,
        room: RoomSize,
    },
    CinemaMode(bool),
    /// Whether the equalizer can currently be used (V2).
    EqAvailable(bool),
    ConnectionPriority(ConnectionPriority),
    NcAmbButton(NcAmbButton),
    Playback(PlaybackState),
    Volume(u8),
    VoiceGuidance(bool),
    VoiceGuidanceVolume(i8),
    /// V2 general setting description. Only on/off settings are reported.
    GeneralSettingInfo {
        slot: u8,
        name: String,
        summary: String,
    },
    GeneralSetting {
        slot: u8,
        on: bool,
    },
    Surround(Surround),
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
            op::AMBIENT_RET | op::AMBIENT_NOTIFY if v2 && p.get(1) == Some(&0x30) => {
                need(p, 3)?;
                NcAmbButton::from_code(p[2])
                    .map(Notification::NcAmbButton)
                    .ok_or(invalid("unknown NC/AMB button setting"))
            }
            op::AMBIENT_RET | op::AMBIENT_NOTIFY => {
                Ok(Notification::AmbientSound(parse_ambient(p, d)?))
            }
            op::V2_EQ_STATUS_RET | op::V2_EQ_STATUS_NOTIFY if v2 => {
                need(p, 3)?;
                Ok(Notification::EqAvailable(p[2] == 0x00))
            }
            op::V2_PLAY_STATUS_RET | op::V2_PLAY_STATUS_NOTIFY if v2 && p.get(1) == Some(&0x01) => {
                need(p, 4)?;
                Ok(Notification::Playback(PlaybackState::from_code(p[3])))
            }
            op::V2_PLAY_PARAM_RET | op::V2_PLAY_PARAM_NOTIFY if v2 && p.get(1) == Some(&0x20) => {
                need(p, 3)?;
                Ok(Notification::Volume(p[2].min(VOLUME_MAX)))
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
            op::V2_AUDIO_STATUS_RET | op::V2_AUDIO_STATUS_NOTIFY
                if v2 && p.get(1) == Some(&0x01) =>
            {
                need(p, 3)?;
                // 0x00 = enabled (usable), 0x01 = disabled.
                Ok(Notification::UpsamplingAvailable(p[2] == 0x00))
            }
            op::AUDIO_UPSAMPLING_RET | op::AUDIO_UPSAMPLING_NOTIFY if v2 => {
                need(p, 3)?;
                match p[1] {
                    0x00 => ConnectionPriority::from_code(p[2])
                        .map(Notification::ConnectionPriority)
                        .ok_or(invalid("unknown connection priority")),
                    0x01 => Ok(Notification::AudioUpsampling(p[2] == 0x01)),
                    0x03 => {
                        need(p, 4)?;
                        let room = RoomSize::from_code(p[3]).ok_or(invalid("unknown room size"))?;
                        Ok(Notification::BgmMode {
                            on: p[2] == 0x00,
                            room,
                        })
                    }
                    0x04 => Ok(Notification::CinemaMode(p[2] == 0x00)),
                    _ => Ok(Notification::Unknown(p.to_vec())),
                }
            }
            op::AUDIO_UPSAMPLING_RET | op::AUDIO_UPSAMPLING_NOTIFY => {
                need(p, 4)?;
                Ok(Notification::AudioUpsampling(p[3] == 0x01))
            }
            op::V1_SURROUND_RET | op::V1_SURROUND_NOTIFY if !v2 => {
                need(p, 3)?;
                if p[1] != 0x01 {
                    return Ok(Notification::Unknown(p.to_vec()));
                }
                Surround::from_code(p[2])
                    .map(Notification::Surround)
                    .ok_or(invalid("unknown surround mode"))
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
            op::V2_GS_RET_CAPABILITY if v2 => parse_gs_info(p),
            op::TOUCH_SENSOR_RET | op::TOUCH_SENSOR_NOTIFY if v2 => {
                need(p, 4)?;
                let slot = gs_slot(p[1]).ok_or(invalid("unknown general setting"))?;
                if p[2] != 0x00 {
                    return Ok(Notification::Unknown(p.to_vec())); // list-type setting
                }
                Ok(Notification::GeneralSetting {
                    slot,
                    on: p[3] == 0x00,
                })
            }
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

    /// Parse a secondary-channel (frame type 0x0E) message.
    pub fn parse_t2(p: &[u8], d: Dialect) -> Result<Self, ParseError> {
        let &opcode = p.first().ok_or(ParseError::Empty)?;
        if !d.is_v2() {
            return Ok(Notification::Unknown(p.to_vec()));
        }
        match opcode {
            op2::VOICE_GUIDANCE_RET | op2::VOICE_GUIDANCE_NOTIFY => {
                need(p, 3)?;
                match p[1] {
                    0x01 => Ok(Notification::VoiceGuidance(p[2] == 0x00)),
                    0x20 => {
                        let (lo, hi) = VOICE_VOLUME_RANGE;
                        let v = i8::from_be_bytes([p[2]]).clamp(lo, hi);
                        Ok(Notification::VoiceGuidanceVolume(v))
                    }
                    _ => Ok(Notification::Unknown(p.to_vec())),
                }
            }
            _ => Ok(Notification::Unknown(p.to_vec())),
        }
    }

    /// Frame type this notification travels on.
    pub fn data_type(&self) -> DataType {
        match self {
            Notification::VoiceGuidance(_) | Notification::VoiceGuidanceVolume(_) => {
                DataType::DataMdrNo2
            }
            _ => DataType::DataMdr,
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
                // V1 requests omit the level outside ambient mode, but the
                // headset reports the remembered level (last byte).
                if !v2 && let Some(last) = p.last_mut() {
                    *last = a.level;
                }
                p
            }
            Notification::Equalizer(eq) => eq_payload(op::EQ_NOTIFY, eq),
            Notification::BgmMode { on, room } => {
                vec![op::AUDIO_UPSAMPLING_NOTIFY, 0x03, v2_flag(*on), room.code()]
            }
            Notification::CinemaMode(on) => vec![op::AUDIO_UPSAMPLING_NOTIFY, 0x04, v2_flag(*on)],
            Notification::EqAvailable(ok) => vec![op::V2_EQ_STATUS_NOTIFY, 0x00, v2_flag(*ok)],
            Notification::ConnectionPriority(prio) => {
                vec![op::AUDIO_UPSAMPLING_NOTIFY, 0x00, prio.code()]
            }
            Notification::NcAmbButton(b) => vec![op::AMBIENT_NOTIFY, 0x30, b.code()],
            Notification::Playback(state) => {
                vec![op::V2_PLAY_STATUS_NOTIFY, 0x01, 0x00, state.code(), 0x00]
            }
            Notification::Volume(v) => vec![op::V2_PLAY_PARAM_NOTIFY, 0x20, *v],
            Notification::VoiceGuidance(on) => vec![op2::VOICE_GUIDANCE_NOTIFY, 0x01, v2_flag(*on)],
            Notification::VoiceGuidanceVolume(v) => {
                vec![op2::VOICE_GUIDANCE_NOTIFY, 0x20, v.to_be_bytes()[0]]
            }
            Notification::Surround(mode) => vec![op::V1_SURROUND_NOTIFY, 0x01, mode.code()],
            Notification::GeneralSettingInfo {
                slot,
                name,
                summary,
            } => {
                let mut p = vec![
                    op::V2_GS_RET_CAPABILITY,
                    gs_slot_code(*slot).unwrap_or(0xD1),
                    0x00,
                    0x01,
                ];
                for text in [name, summary] {
                    let bytes = &text.as_bytes()[..text.len().min(GS_TEXT_MAX)];
                    p.push(u8::try_from(bytes.len()).unwrap_or(0));
                    p.extend_from_slice(bytes);
                }
                p
            }
            Notification::GeneralSetting { slot, on } => {
                vec![
                    op::TOUCH_SENSOR_NOTIFY,
                    gs_slot_code(*slot).unwrap_or(0xD1),
                    0x00,
                    v2_flag(*on),
                ]
            }
            Notification::UpsamplingAvailable(ok) => {
                vec![op::V2_AUDIO_STATUS_NOTIFY, 0x01, v2_flag(*ok)]
            }
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
    const XM6_D: Dialect = Dialect {
        version: ProtocolVersion::V2,
        ambient: AmbientType::NoiseAdaptive,
    };
    const ALL_DIALECTS: [Dialect; 4] = [Dialect::V1, XM5, WF_XM5, XM6_D];

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
                ..AmbientSound::default()
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
            ..AmbientSound::default()
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
            ..AmbientSound::default()
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
    fn xm6_noise_adaptive_ambient_format() {
        const XM6: Dialect = Dialect {
            version: ProtocolVersion::V2,
            ambient: AmbientType::NoiseAdaptive,
        };
        // [69, 19, status, on, nc/asm, voice, level, auto, sensitivity]
        let parsed =
            Notification::parse(&[0x69, 0x19, 0x01, 0x01, 0x01, 0x01, 0x0C, 0x01, 0x02], XM5)
                .unwrap();
        assert_eq!(
            parsed,
            Notification::AmbientSound(AmbientSound {
                mode: AmbientMode::AmbientSound,
                level: 12,
                focus_on_voice: true,
                auto_ambient: true,
                auto_sensitivity: AutoAmbientSensitivity::Low,
            })
        );
        assert_eq!(
            Notification::ambient_type_of(&[0x69, 0x19, 0x01]),
            Some(AmbientType::NoiseAdaptive)
        );
        let nc = Request::SetAmbientSound {
            value: AmbientSound::default(),
            adjusting: false,
        };
        assert_eq!(
            nc.payload(XM6).unwrap(),
            vec![0x68, 0x19, 0x01, 0x01, 0x00, 0x00, 20, 0x00, 0x00]
        );
        let dragging = Request::SetAmbientSound {
            value: AmbientSound {
                mode: AmbientMode::AmbientSound,
                level: 5,
                ..AmbientSound::default()
            },
            adjusting: true,
        };
        assert_eq!(
            dragging.payload(XM6).unwrap(),
            vec![0x68, 0x19, 0x00, 0x01, 0x01, 0x00, 5, 0x00, 0x00]
        );
        assert_eq!(
            Request::decode(&dragging.payload(XM6).unwrap(), XM6),
            Some(dragging)
        );
        assert_eq!(
            Request::GetAmbientSound.payload(XM6).unwrap(),
            vec![0x66, 0x19]
        );
    }

    #[test]
    fn ten_band_equalizer_matches_reference() {
        // 10-band (WH-1000XM6): wire value = step + 6, range -6..=6.
        let flat = Equalizer::ten_band(EqPreset::Manual, [0; 10]);
        assert_eq!(
            Request::SetEqualizerBands(flat).payload(XM5).unwrap(),
            vec![0x58, 0x00, 0xA0, 0x0A, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6]
        );
        let mut tuned = flat;
        tuned.bands[0] = 3;
        tuned.bands[9] = -2;
        assert_eq!(
            Request::SetEqualizerBands(tuned).payload(XM5).unwrap(),
            vec![0x58, 0x00, 0xA0, 0x0A, 0x09, 6, 6, 6, 6, 6, 6, 6, 6, 0x04]
        );
        let parsed = Notification::parse(
            &[0x57, 0x00, 0x30, 0x0A, 6, 7, 8, 9, 10, 11, 12, 0, 1, 2],
            XM5,
        )
        .unwrap();
        assert_eq!(
            parsed,
            Notification::Equalizer(Equalizer::ten_band(
                EqPreset::Heavy,
                [0, 1, 2, 3, 4, 5, 6, -6, -5, -4]
            ))
        );
        // Preset-only reply (no bands) still parses.
        assert!(matches!(
            Notification::parse(&[0x59, 0x00, 0x31, 0x00], XM5).unwrap(),
            Notification::Equalizer(Equalizer {
                preset: EqPreset::Clear,
                ..
            })
        ));
        assert_eq!(EqLayout::TenBand.presets().len(), 8);
    }

    #[test]
    fn general_settings() {
        assert_eq!(
            Request::GetGeneralSettingInfo(3).payload(XM5).unwrap(),
            vec![0xD0, 0xD3, 0x01]
        );
        assert_eq!(
            Request::GetGeneralSetting(3).payload(XM5).unwrap(),
            vec![0xD6, 0xD3]
        );
        assert_eq!(
            Request::SetGeneralSetting { slot: 3, on: false }
                .payload(XM5)
                .unwrap(),
            vec![0xD8, 0xD3, 0x00, 0x01]
        );
        assert_eq!(Request::GetGeneralSetting(9).payload(XM5), None);
        assert_eq!(Request::GetGeneralSetting(3).payload(Dialect::V1), None);
        // RET capability: [D1, slot, bool, enum-name, len, subject, len, summary]
        let mut ret = vec![0xD1, 0xD3, 0x00, 0x01, 19];
        ret.extend_from_slice(b"TOUCH_PANEL_SETTING");
        ret.push(0);
        assert_eq!(
            Notification::parse(&ret, XM5).unwrap(),
            Notification::GeneralSettingInfo {
                slot: 3,
                name: "Touch panel".into(),
                summary: String::new()
            }
        );
        // List-type settings are not offered (yet).
        assert!(matches!(
            Notification::parse(&[0xD1, 0xD2, 0x01, 0x00, 0, 0], XM5).unwrap(),
            Notification::Unknown(_)
        ));
        // Truncated text lengths are errors, not panics.
        assert!(Notification::parse(&[0xD1, 0xD3, 0x00, 0x01, 50, b'A'], XM5).is_err());
        assert_eq!(
            Notification::parse(&[0xD7, 0xD3, 0x00, 0x00], XM5).unwrap(),
            Notification::GeneralSetting { slot: 3, on: true }
        );
        // Same opcodes on V1 still mean the old touch sensor.
        assert_eq!(
            Notification::parse(&[0xD7, 0xD2, 0x01, 0x01], Dialect::V1).unwrap(),
            Notification::TouchSensor(true)
        );
        for r in [
            Request::GetGeneralSettingInfo(1),
            Request::GetGeneralSetting(4),
            Request::SetGeneralSetting { slot: 2, on: true },
        ] {
            assert_eq!(Request::decode(&r.payload(XM5).unwrap(), XM5), Some(r));
        }
        let n = Notification::GeneralSettingInfo {
            slot: 2,
            name: "Multipoint".into(),
            summary: "Connect two devices".into(),
        };
        assert_eq!(Notification::parse(&n.payload(XM5), XM5).unwrap(), n);
    }

    #[test]
    fn dsee_availability_from_a_real_wh1000xm6() {
        // Captured from a WH-1000XM6: Cinema on, then off.
        assert_eq!(
            Notification::parse(&[0xE5, 0x01, 0x01], XM5).unwrap(),
            Notification::UpsamplingAvailable(false)
        );
        assert_eq!(
            Notification::parse(&[0xE5, 0x01, 0x00], XM5).unwrap(),
            Notification::UpsamplingAvailable(true)
        );
        // DSEE on/off itself still parses.
        assert_eq!(
            Notification::parse(&[0xE9, 0x01, 0x01], XM5).unwrap(),
            Notification::AudioUpsampling(true)
        );
    }

    #[test]
    fn new_v2_features_match_reference() {
        let b = |r: Request| r.payload(XM5).unwrap();
        assert_eq!(b(Request::GetConnectionPriority), vec![0xE6, 0x00]);
        assert_eq!(
            b(Request::SetConnectionPriority(
                ConnectionPriority::StableConnection
            )),
            vec![0xE8, 0x00, 0x01]
        );
        assert_eq!(
            b(Request::SetNcAmbButton(NcAmbButton::NcAmbient)),
            vec![0x68, 0x30, 0x02]
        );
        assert_eq!(
            b(Request::Playback(PlaybackControl::Play)),
            vec![0xA4, 0x01, 0x00, 0x07]
        );
        assert_eq!(
            b(Request::Playback(PlaybackControl::Pause)),
            vec![0xA4, 0x01, 0x00, 0x01]
        );
        assert_eq!(
            b(Request::Playback(PlaybackControl::Next)),
            vec![0xA4, 0x01, 0x00, 0x02]
        );
        assert_eq!(
            b(Request::Playback(PlaybackControl::Previous)),
            vec![0xA4, 0x01, 0x00, 0x03]
        );
        assert_eq!(b(Request::SetVolume(15)), vec![0xA8, 0x20, 0x0F]);
        assert_eq!(b(Request::SetVolume(99)), vec![0xA8, 0x20, 30]);
        assert_eq!(b(Request::GetEqAvailability), vec![0x52, 0x00]);
        // Voice guidance travels on the secondary channel; 0x00 = on.
        assert_eq!(
            Request::SetVoiceGuidance(false).data_type(),
            DataType::DataMdrNo2
        );
        assert_eq!(b(Request::SetVoiceGuidance(true)), vec![0x48, 0x01, 0x00]);
        assert_eq!(b(Request::SetVoiceGuidance(false)), vec![0x48, 0x01, 0x01]);
        assert_eq!(
            b(Request::SetVoiceGuidanceVolume(1)),
            vec![0x48, 0x20, 0x01, 0x00]
        );
        assert_eq!(
            b(Request::SetVoiceGuidanceVolume(-2)),
            vec![0x48, 0x20, 0xFE, 0x00]
        );
        // None of these exist on V1 headsets.
        assert_eq!(Request::SetVolume(3).payload(Dialect::V1), None);
        assert_eq!(Request::SetVoiceGuidance(true).payload(Dialect::V1), None);

        // Replies
        assert_eq!(
            Notification::parse(&[0xA5, 0x01, 0x00, 0x01, 0x00], XM5).unwrap(),
            Notification::Playback(PlaybackState::Playing)
        );
        assert_eq!(
            Notification::parse(&[0xA9, 0x20, 0x0F], XM5).unwrap(),
            Notification::Volume(15)
        );
        assert_eq!(
            Notification::parse(&[0xE9, 0x00, 0x01], XM5).unwrap(),
            Notification::ConnectionPriority(ConnectionPriority::StableConnection)
        );
        assert_eq!(
            Notification::parse(&[0x69, 0x30, 0x03], XM5).unwrap(),
            Notification::NcAmbButton(NcAmbButton::NcOff)
        );
        assert_eq!(
            Notification::parse(&[0x55, 0x00, 0x01], XM5).unwrap(),
            Notification::EqAvailable(false)
        );
        // RET of voice guidance carries a language byte too.
        assert_eq!(
            Notification::parse_t2(&[0x47, 0x01, 0x00, 0x01], XM5).unwrap(),
            Notification::VoiceGuidance(true)
        );
        assert_eq!(
            Notification::parse_t2(&[0x49, 0x20, 0xFF], XM5).unwrap(),
            Notification::VoiceGuidanceVolume(-1)
        );
        // 0x46-0x49 mean surround on the main channel of V1 headsets: no mix-up.
        assert_eq!(
            Notification::parse(&[0x49, 0x01, 0x04], Dialect::V1).unwrap(),
            Notification::Surround(Surround::Club)
        );
    }

    #[test]
    fn new_requests_roundtrip_through_decode() {
        let t1 = [
            Request::GetConnectionPriority,
            Request::SetConnectionPriority(ConnectionPriority::StableConnection),
            Request::GetNcAmbButton,
            Request::SetNcAmbButton(NcAmbButton::AmbientOff),
            Request::GetPlayback,
            Request::Playback(PlaybackControl::Next),
            Request::GetVolume,
            Request::SetVolume(21),
            Request::GetEqAvailability,
            Request::SetEqualizerBands(Equalizer::ten_band(
                EqPreset::Custom1,
                [1, 2, 3, 4, 5, 6, -1, -2, -3, -4],
            )),
        ];
        for r in t1 {
            assert_eq!(Request::decode(&r.payload(XM5).unwrap(), XM5), Some(r));
        }
        let t2 = [
            Request::GetVoiceGuidance,
            Request::SetVoiceGuidance(false),
            Request::GetVoiceGuidanceVolume,
            Request::SetVoiceGuidanceVolume(-2),
        ];
        for r in t2 {
            assert_eq!(Request::decode_t2(&r.payload(XM5).unwrap(), XM5), Some(r));
        }
    }

    #[test]
    fn listening_mode_bytes_match_reference() {
        // AudioParamBGMMode / AudioParamUpmixCinema; 0x00 = enable, 0x01 = disable.
        let bgm = Request::SetBgmMode {
            on: true,
            room: RoomSize::Middle,
        };
        assert_eq!(bgm.payload(XM5).unwrap(), vec![0xE8, 0x03, 0x00, 0x01]);
        let std = Request::SetBgmMode {
            on: false,
            room: RoomSize::Small,
        };
        assert_eq!(std.payload(XM5).unwrap(), vec![0xE8, 0x03, 0x01, 0x00]);
        assert_eq!(
            Request::SetCinemaMode(true).payload(XM5).unwrap(),
            vec![0xE8, 0x04, 0x00]
        );
        assert_eq!(
            Request::GetCinemaMode.payload(XM5).unwrap(),
            vec![0xE6, 0x04]
        );
        assert_eq!(Request::SetCinemaMode(true).payload(Dialect::V1), None);
        assert_eq!(
            Notification::parse(&[0xE9, 0x03, 0x00, 0x02], XM5).unwrap(),
            Notification::BgmMode {
                on: true,
                room: RoomSize::Large
            }
        );
        assert_eq!(
            Notification::parse(&[0xE7, 0x04, 0x01], XM5).unwrap(),
            Notification::CinemaMode(false)
        );
        // DSEE still parses next to them.
        assert_eq!(
            Notification::parse(&[0xE7, 0x01, 0x01], XM5).unwrap(),
            Notification::AudioUpsampling(true)
        );
    }

    #[test]
    fn v1_surround_matches_gadgetbridge() {
        assert_eq!(
            Request::SetSurround(Surround::Arena)
                .payload(Dialect::V1)
                .unwrap(),
            vec![0x48, 0x01, 0x02]
        );
        assert_eq!(Request::SetSurround(Surround::Arena).payload(XM5), None);
        assert_eq!(
            Notification::parse(&[0x49, 0x01, 0x04], Dialect::V1).unwrap(),
            Notification::Surround(Surround::Club)
        );
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
                    ..AmbientSound::default()
                },
                adjusting: false,
            },
            Request::SetAmbientSound {
                value: AmbientSound {
                    mode: AmbientMode::NoiseCancelling,
                    level: 0,
                    focus_on_voice: false,
                    ..AmbientSound::default()
                },
                adjusting: false,
            },
            Request::SetEqualizerPreset(EqPreset::Vocal),
            Request::SetEqualizerBands(Equalizer::classic(EqPreset::Manual, [1, -2, 3, -4, 5, 10])),
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
                // The 0x19 format always sends a level of at least 1.
                let mut expected = r.clone();
                if let Request::SetAmbientSound { value, .. } = &mut expected
                    && d.ambient == AmbientType::NoiseAdaptive
                    && d.is_v2()
                {
                    value.level = value.level.max(AMBIENT_LEVEL_MIN);
                }
                assert_eq!(
                    Request::decode(&bytes, d).as_ref(),
                    Some(&expected),
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
            Notification::Equalizer(Equalizer::classic(
                EqPreset::Custom1,
                [10, -10, 0, 3, -4, 7],
            )),
            Notification::Equalizer(Equalizer::ten_band(
                EqPreset::Manual,
                [3, -6, 0, 1, 2, 6, -1, 0, 4, -2],
            )),
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
                ..AmbientSound::default()
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
            ..AmbientSound::default()
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
        let wild = Request::SetEqualizerBands(Equalizer::classic(
            EqPreset::Manual,
            [127, -128, 0, 0, 0, 0],
        ));
        assert_eq!(&wild.payload(XM5).unwrap()[4..6], &[20, 0]);
        let wild10 = Request::SetEqualizerBands(Equalizer::ten_band(
            EqPreset::Manual,
            [100, -100, 0, 0, 0, 0, 0, 0, 0, 0],
        ));
        assert_eq!(&wild10.payload(XM5).unwrap()[4..6], &[12, 0]);
    }

    #[test]
    fn malformed_input_never_panics() {
        for d in ALL_DIALECTS {
            for opcode in 0..=255u8 {
                for len in 0..10 {
                    let mut p = vec![opcode];
                    p.extend(std::iter::repeat_n(0x09, len));
                    let _ = Notification::parse(&p, d);
                    let _ = Notification::parse_t2(&p, d);
                    let _ = Request::decode(&p, d);
                    let _ = Request::decode_t2(&p, d);
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
