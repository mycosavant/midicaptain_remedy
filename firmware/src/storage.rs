//! Flash-backed settings store.
//!
//! Persists the handful of user-tunable settings the firmware needs to
//! survive a power cycle: MIDI channel, display/LED brightness, and the
//! two expression-pedal calibrations. It replaces the CircuitPython NVM
//! hack (`remedy/lib/menu.py::_save_calibration`), which abused the
//! RP2040's small NVM region because CircuitPython's filesystem was
//! read-only at runtime. We own the flash directly, so we can do this
//! properly: a wear-levelling key-value store in a dedicated flash region.
//!
//! ## Where it lives
//!
//! A 64 KB region (16 x 4 KB erase sectors) at the very TOP of the 2 MB
//! QSPI flash, `0x1F_0000 .. 0x20_0000` (flash-relative offsets; the XIP
//! window base `0x1000_0000` is added by the hardware). `memory.x` shrinks
//! the `FLASH` linker region by exactly this much so the firmware image can
//! never overlap the store — the two ranges are disjoint by construction,
//! not by hoping the binary stays small. Keep the constants here in sync
//! with `memory.x`.
//!
//! ## How it works
//!
//! [`sequential-storage`](https://docs.rs/sequential-storage)'s `map` is a
//! log-structured key-value store purpose-built for NOR flash: new writes
//! append, old values are superseded, and sectors are garbage-collected and
//! erased as they fill. That spreads erase wear across the whole region
//! instead of hammering one sector — exactly what a settings store that's
//! rewritten on every calibration needs.
//!
//! The crate's API is async (it consumes the `embedded-storage-async`
//! `NorFlash` traits). The RP2040's flash erase/program are inherently
//! synchronous, though — they execute from RAM with XIP paused — so rather
//! than reserve a DMA channel and wire up its interrupt just to drive
//! background reads, we wrap embassy-rp's *blocking* `Flash` in the tiny
//! [`AsyncFlash`] shim below. Its async methods simply call the blocking
//! ones and resolve immediately. Net effect: the store touches no DMA
//! subsystem and adds no interrupt-handler requirement to the application.
//!
//! ## Encoding
//!
//! One `u8` key per setting. Scalars (`u8`) are stored natively; each
//! pedal's calibration is packed `min:max` into a single `u32`
//! (`min << 16 | max`) and stored under one key, so a pedal's min and max
//! are always written and read together — a power loss can never leave a
//! fresh `min` paired with a stale `max`.
//!
//! ## API shape
//!
//! A plain async accessor struct (no task / channel). The settings menu and
//! the expression-pedal subsystem call it directly at their own infrequent
//! save/load points. [`load`](Storage::load) is resilient: a key that was
//! never written — or a one-off read error — yields the documented default
//! rather than failing the boot. Writes are fallible so the UI can report a
//! genuine save failure.

use core::ops::Range;

use crate::config::{self, RuntimeConfig};
use embassy_rp::Peri;
use embassy_rp::flash::{Blocking, Flash};
use embassy_rp::peripherals::FLASH;
use embedded_storage::nor_flash::{
    NorFlash as BlockingNorFlash, ReadNorFlash as BlockingReadNorFlash,
};
use embedded_storage_async::nor_flash::{ErrorType, NorFlash, ReadNorFlash};
use sequential_storage::cache::NoCache;
use sequential_storage::map::{MapConfig, MapStorage};

// ── Flash geometry ───────────────────────────────────────────────────────

/// Total QSPI flash on the RP2040 board (2 MB). This is the bounds-check
/// limit handed to `embassy_rp::flash::Flash` as its `FLASH_SIZE` const
/// generic; all offsets the driver accepts are `0..FLASH_SIZE`.
pub const FLASH_SIZE: usize = 2 * 1024 * 1024;

/// Size of the reserved key-value region (64 KB = 16 erase sectors).
pub const CONFIG_REGION_SIZE: u32 = 64 * 1024;

/// One-past-the-end of the region — the very top of flash.
pub const CONFIG_REGION_END: u32 = FLASH_SIZE as u32; // 0x0020_0000

/// Start of the region. 4 KB-aligned (a multiple of the erase size), as
/// `sequential-storage` requires.
pub const CONFIG_REGION_START: u32 = CONFIG_REGION_END - CONFIG_REGION_SIZE; // 0x001F_0000

// ── Keys ─────────────────────────────────────────────────────────────────
// Stable on-flash identifiers. NEVER reuse a number for a different meaning
// across firmware versions — old values with that key may still be in flash.
mod key {
    pub const MIDI_CHANNEL: u8 = 0x01;
    pub const DISPLAY_BRIGHTNESS: u8 = 0x02;
    pub const LED_BRIGHTNESS: u8 = 0x03;
    /// Packed `min:max` calibration, indexed by pedal (`0` or `1`).
    pub const PEDAL_CAL: [u8; 2] = [0x10, 0x11];
    /// Serialized user [`crate::config::RuntimeConfig`] (postcard blob). A
    /// single large value, separate from the scalar settings above.
    pub const CONFIG: u8 = 0x20;
}

/// Recommended scratch-buffer length for [`Storage::store_config`] /
/// [`Storage::load_config`]. The caller owns the buffer (it is too large to
/// keep inside [`Storage`] for the program's lifetime); `store` splits it into
/// a serialize half and a framing half, so it must be `≥ 2 ×` the largest
/// serialized config. 8 KiB comfortably covers [`crate::config::MAX_PAGES`].
pub const CONFIG_SCRATCH_LEN: usize = 8 * 1024;

// ── Defaults ─────────────────────────────────────────────────────────────
// Returned when a key has never been stored. These mirror the "safe" values
// the CircuitPython firmware fell back to.

/// MIDI channel, 1-based (the menu clamps this to `1..=16`).
pub const DEFAULT_MIDI_CHANNEL: u8 = 1;
/// Display backlight brightness, percent.
pub const DEFAULT_DISPLAY_BRIGHTNESS: u8 = 80;
/// LED brightness, percent.
pub const DEFAULT_LED_BRIGHTNESS: u8 = 80;

/// Calibration for one expression pedal: the raw ADC readings that map to
/// the bottom and top of the controller's `0..=127` output range.
#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format)]
pub struct PedalCal {
    /// Raw ADC value at the heel (mapped to output `0`).
    pub min: u16,
    /// Raw ADC value at the toe (mapped to output `127`).
    pub max: u16,
}

impl PedalCal {
    /// Uncalibrated default. The store is calibration-width-agnostic — it
    /// just round-trips two `u16`s — so this uses the `0xFFFF` blank-NVM
    /// sentinel the CP firmware used. When fed to
    /// [`crate::hal::expression::PedalProcessor::set_calibration`], the
    /// RP2040 ADC's 12-bit ceiling clamps `max` to
    /// [`crate::hal::expression::ADC_FULL_SCALE`] (4095), yielding exactly
    /// the full-span identity mapping that module's own
    /// `Calibration::DEFAULT` defines. (The two defaults therefore agree
    /// after the clamp — do not "fix" one to match the other's literal.)
    pub const DEFAULT: Self = Self {
        min: 0,
        max: u16::MAX,
    };

    /// Pack into the `u32` stored under one key, so min and max are atomic.
    const fn encode(self) -> u32 {
        ((self.min as u32) << 16) | self.max as u32
    }

    /// Inverse of [`encode`](Self::encode).
    const fn decode(raw: u32) -> Self {
        Self {
            min: (raw >> 16) as u16,
            max: raw as u16,
        }
    }
}

impl Default for PedalCal {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The complete persisted settings snapshot.
#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format)]
pub struct Settings {
    /// MIDI channel, 1-based.
    pub midi_channel: u8,
    /// Display backlight brightness, percent.
    pub display_brightness: u8,
    /// LED brightness, percent.
    pub led_brightness: u8,
    /// Calibration per pedal: index `0` = expression 1, `1` = expression 2.
    pub pedal_cal: [PedalCal; 2],
}

impl Settings {
    /// The factory defaults, used when flash is blank.
    pub const DEFAULT: Self = Self {
        midi_channel: DEFAULT_MIDI_CHANNEL,
        display_brightness: DEFAULT_DISPLAY_BRIGHTNESS,
        led_brightness: DEFAULT_LED_BRIGHTNESS,
        pedal_cal: [PedalCal::DEFAULT, PedalCal::DEFAULT],
    };
}

impl Default for Settings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Errors a settings access can surface.
#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format)]
pub enum StorageError {
    /// The underlying flash operation failed, or the store is full/corrupt.
    /// The failing operation is logged via `defmt` at the call site.
    Backend,
    /// `pedal` index was neither `0` nor `1`.
    BadPedal,
}

// ── Blocking → async flash shim ──────────────────────────────────────────

/// Concrete blocking flash over the whole 2 MB QSPI.
type BlockingFlash = Flash<'static, FLASH, Blocking, FLASH_SIZE>;

/// Adapts the blocking [`embassy_rp::flash::Flash`] to the async
/// `embedded-storage` `NorFlash` traits `sequential-storage` consumes.
///
/// RP2040 flash erase/program are synchronous (they run from RAM with XIP
/// paused), so there is nothing to truly await: the async methods call the
/// blocking ones and return immediately. This keeps the store off the DMA
/// subsystem — no channel reserved, no DMA interrupt to wire up — at the
/// cost of briefly blocking the executor during an erase (tens of ms, only
/// on an actual save). For a settings store that is the right trade.
struct AsyncFlash(BlockingFlash);

impl ErrorType for AsyncFlash {
    type Error = embassy_rp::flash::Error;
}

impl ReadNorFlash for AsyncFlash {
    const READ_SIZE: usize = <BlockingFlash as BlockingReadNorFlash>::READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        BlockingReadNorFlash::read(&mut self.0, offset, bytes)
    }

    fn capacity(&self) -> usize {
        BlockingReadNorFlash::capacity(&self.0)
    }
}

impl NorFlash for AsyncFlash {
    const WRITE_SIZE: usize = <BlockingFlash as BlockingNorFlash>::WRITE_SIZE;
    const ERASE_SIZE: usize = <BlockingFlash as BlockingNorFlash>::ERASE_SIZE;

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        BlockingNorFlash::write(&mut self.0, offset, bytes)
    }

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        BlockingNorFlash::erase(&mut self.0, from, to)
    }
}

// ── Public accessor ──────────────────────────────────────────────────────

/// Async accessor for the persisted [`Settings`].
///
/// Construct once at boot with [`Storage::new`], then call the typed
/// load/store methods. It owns the flash peripheral and a scan buffer (sized
/// to the largest map item — the config blob); it spawns nothing.
pub struct Storage {
    map: MapStorage<u8, AsyncFlash, NoCache>,
    /// Scratch buffer the scalar settings accessors serialize *and scan*
    /// through.
    ///
    /// `sequential-storage`'s map is log-structured: `fetch_item` (and the GC
    /// that `store_item` can trigger) walk **every** item in the active
    /// page(s), reading each one into this buffer to compare its key — see the
    /// crate's own note, *"the data buffer must be long enough to hold the
    /// longest serialized data of your Key + Value types combined."* So this is
    /// **not** sized for a scalar; it must hold the **largest item in the whole
    /// map**, which is the user-config blob ([`key::CONFIG`]), or a scalar read
    /// will fault with `BufferTooSmall` the moment a config has been stored —
    /// silently reverting every saved setting to its default. [`BUF_LEN`] is
    /// therefore derived from [`config::MAX_SERIALIZED_LEN`] and pinned to it by
    /// a compile-time assertion below.
    ///
    /// [`BUF_LEN`]: Self::BUF_LEN
    buf: [u8; Self::BUF_LEN],
}

// Pin the scalar scan buffer to the config model: it must be able to hold the
// largest item in the map (the config blob) plus its key, or scalar reads fault
// with `BufferTooSmall` once any config is stored. If `config::MAX_PAGES` (or a
// cap) ever grows past what this buffer covers, this fails the build instead of
// silently re-introducing the "config push wipes saved settings" bug.
const _: () = assert!(
    Storage::BUF_LEN >= Storage::MAX_MAP_ITEM_LEN,
    "Storage::buf is smaller than the largest map item (config blob); \
     scalar settings reads would fault with BufferTooSmall after a config store"
);

// `store_config` serializes the config into the *first half* of the caller's
// scratch ([`CONFIG_SCRATCH_LEN`]), so the worst-case blob must fit in half of
// it. This also keeps one map item comfortably under a single 4 KB flash erase
// sector — `sequential-storage`'s per-item ceiling. If a config cap
// (`MAX_PAGES` / `MAX_CYCLES` / `MAX_STEPS` / a label cap) ever grows the blob
// past this, the build fails here instead of `store_config` failing at runtime.
const _: () = assert!(
    2 * config::MAX_SERIALIZED_LEN <= CONFIG_SCRATCH_LEN,
    "config::MAX_SERIALIZED_LEN exceeds half the config scratch buffer; \
     store_config could not serialize the worst-case config (raise CONFIG_SCRATCH_LEN \
     only if it still fits a 4 KB flash sector, else reduce a config cap)"
);

impl Storage {
    /// The largest item the key-value map can hold: a 1-byte [`u8`] key plus the
    /// largest serialized config value ([`config::MAX_SERIALIZED_LEN`]). The
    /// config blob is by far the biggest value; the scalars are a few bytes. Any
    /// buffer used to traverse the map must be at least this large.
    const MAX_MAP_ITEM_LEN: usize = 1 + config::MAX_SERIALIZED_LEN;

    /// Length of the scalar accessors' scan buffer. Must be `≥`
    /// [`MAX_MAP_ITEM_LEN`](Self::MAX_MAP_ITEM_LEN), because every map traversal
    /// reads each item it passes through this buffer (see [`Storage::buf`]).
    ///
    /// `+ 16` is headroom for `read_item` rounding the item length up to the
    /// flash write-word size (the RP2040 program word is 4 bytes). The `const`
    /// assertion above the `impl` fails the build if this ever drops below the
    /// proven bound — e.g. if [`config::MAX_PAGES`] grows.
    const BUF_LEN: usize = Self::MAX_MAP_ITEM_LEN + 16;
    const RANGE: Range<u32> = CONFIG_REGION_START..CONFIG_REGION_END;

    /// Bind the store to the flash peripheral.
    ///
    /// Validates the region geometry (alignment, minimum size, word size)
    /// and panics if it is wrong — that can only happen from an editing
    /// mistake to the constants above, so failing loudly at boot is correct.
    pub fn new(flash: Peri<'static, FLASH>) -> Self {
        let flash = AsyncFlash(Flash::new_blocking(flash));
        let config = MapConfig::new(Self::RANGE);
        Self {
            map: MapStorage::new(flash, config, NoCache::new()),
            buf: [0; Self::BUF_LEN],
        }
    }

    /// Read every setting, substituting defaults for any that are unset or
    /// fail to read. Never fails — intended for the boot path.
    pub async fn load(&mut self) -> Settings {
        Settings {
            midi_channel: self.get_u8(key::MIDI_CHANNEL, DEFAULT_MIDI_CHANNEL).await,
            display_brightness: self
                .get_u8(key::DISPLAY_BRIGHTNESS, DEFAULT_DISPLAY_BRIGHTNESS)
                .await,
            led_brightness: self
                .get_u8(key::LED_BRIGHTNESS, DEFAULT_LED_BRIGHTNESS)
                .await,
            pedal_cal: [
                self.load_pedal_cal(0).await.unwrap_or(PedalCal::DEFAULT),
                self.load_pedal_cal(1).await.unwrap_or(PedalCal::DEFAULT),
            ],
        }
    }

    /// Persist every field of `settings`. Stops at the first failure.
    pub async fn store(&mut self, settings: &Settings) -> Result<(), StorageError> {
        self.set_midi_channel(settings.midi_channel).await?;
        self.set_display_brightness(settings.display_brightness)
            .await?;
        self.set_led_brightness(settings.led_brightness).await?;
        self.store_pedal_cal(0, settings.pedal_cal[0]).await?;
        self.store_pedal_cal(1, settings.pedal_cal[1]).await?;
        Ok(())
    }

    /// Persist the MIDI channel (caller clamps to `1..=16`).
    pub async fn set_midi_channel(&mut self, channel: u8) -> Result<(), StorageError> {
        self.set_u8(key::MIDI_CHANNEL, channel).await
    }

    /// Persist the display backlight brightness (percent).
    pub async fn set_display_brightness(&mut self, percent: u8) -> Result<(), StorageError> {
        self.set_u8(key::DISPLAY_BRIGHTNESS, percent).await
    }

    /// Persist the LED brightness (percent).
    pub async fn set_led_brightness(&mut self, percent: u8) -> Result<(), StorageError> {
        self.set_u8(key::LED_BRIGHTNESS, percent).await
    }

    /// Load one pedal's calibration. Returns [`PedalCal::DEFAULT`] if it was
    /// never stored; errors only on a real flash failure.
    pub async fn load_pedal_cal(&mut self, pedal: u8) -> Result<PedalCal, StorageError> {
        let k = Self::pedal_key(pedal)?;
        match self.map.fetch_item::<u32>(&mut self.buf, &k).await {
            Ok(Some(raw)) => Ok(PedalCal::decode(raw)),
            Ok(None) => Ok(PedalCal::DEFAULT),
            Err(_) => {
                defmt::warn!("storage: read pedal {} cal failed", pedal);
                Err(StorageError::Backend)
            }
        }
    }

    /// Persist one pedal's calibration atomically.
    pub async fn store_pedal_cal(&mut self, pedal: u8, cal: PedalCal) -> Result<(), StorageError> {
        let k = Self::pedal_key(pedal)?;
        let raw = cal.encode();
        self.map.store_item(&mut self.buf, &k, &raw).await.map_err(|_| {
            defmt::warn!("storage: write pedal {} cal failed", pedal);
            StorageError::Backend
        })
    }

    /// Erase the whole region back to factory defaults. The next
    /// [`load`](Self::load) returns [`Settings::DEFAULT`].
    pub async fn factory_reset(&mut self) -> Result<(), StorageError> {
        self.map.erase_all().await.map_err(|_| {
            defmt::warn!("storage: factory reset failed");
            StorageError::Backend
        })
    }

    // ── user config blob ──────────────────────────────────────────────────

    /// Persist a user [`RuntimeConfig`] as a postcard blob under one key.
    ///
    /// `scratch` (≥ [`CONFIG_SCRATCH_LEN`]) is split in half: the config is
    /// serialized into the first half, and the map frames the resulting blob
    /// through the second. Both halves must hold the blob, hence the `2×`.
    pub async fn store_config(
        &mut self,
        cfg: &RuntimeConfig,
        scratch: &mut [u8],
    ) -> Result<(), StorageError> {
        let (ser, frame) = scratch.split_at_mut(scratch.len() / 2);
        let blob = config::serialize(cfg, ser).map_err(|_| {
            defmt::warn!("storage: config serialize failed (scratch too small?)");
            StorageError::Backend
        })?;
        let len = blob.len();
        self.map
            .store_item(frame, &key::CONFIG, &blob)
            .await
            .map_err(|_| {
                defmt::warn!("storage: config write failed");
                StorageError::Backend
            })?;
        defmt::info!("storage: config stored ({=usize} bytes)", len);
        Ok(())
    }

    /// Load the user config, falling back to
    /// [`RuntimeConfig::default_config`] when none is stored, the read fails,
    /// or the blob is corrupt/empty. Never fails — intended for the boot path.
    /// `scratch` must be large enough to hold the stored blob
    /// (≥ [`CONFIG_SCRATCH_LEN`]).
    pub async fn load_config(&mut self, scratch: &mut [u8]) -> RuntimeConfig {
        match self.map.fetch_item::<&[u8]>(scratch, &key::CONFIG).await {
            Ok(Some(bytes)) => match config::deserialize(bytes) {
                Ok(cfg) if cfg.page_count() > 0 => cfg,
                Ok(_) => {
                    defmt::warn!("storage: stored config has no pages; using default");
                    RuntimeConfig::default_config()
                }
                Err(_) => {
                    defmt::warn!("storage: stored config corrupt; using default");
                    RuntimeConfig::default_config()
                }
            },
            Ok(None) => RuntimeConfig::default_config(),
            Err(_) => {
                defmt::warn!("storage: config read failed; using default");
                RuntimeConfig::default_config()
            }
        }
    }

    // ── internals ────────────────────────────────────────────────────────

    fn pedal_key(pedal: u8) -> Result<u8, StorageError> {
        key::PEDAL_CAL
            .get(pedal as usize)
            .copied()
            .ok_or(StorageError::BadPedal)
    }

    async fn get_u8(&mut self, k: u8, default: u8) -> u8 {
        match self.map.fetch_item::<u8>(&mut self.buf, &k).await {
            Ok(Some(v)) => v,
            Ok(None) => default,
            Err(_) => {
                defmt::warn!("storage: read key {} failed; using default", k);
                default
            }
        }
    }

    async fn set_u8(&mut self, k: u8, value: u8) -> Result<(), StorageError> {
        self.map.store_item(&mut self.buf, &k, &value).await.map_err(|_| {
            defmt::warn!("storage: write key {} failed", k);
            StorageError::Backend
        })
    }
}
