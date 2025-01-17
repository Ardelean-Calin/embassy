use core::convert::TryInto;
use core::ptr::write_volatile;

use atomic_polyfill::{fence, Ordering};

use super::{FlashRegion, FlashSector, FLASH_REGIONS, WRITE_SIZE};
use crate::flash::Error;
use crate::pac;

pub const fn get_flash_regions() -> &'static [&'static FlashRegion] {
    &FLASH_REGIONS
}

pub(crate) unsafe fn lock() {
    pac::FLASH.cr().modify(|w| w.set_lock(true));
}

pub(crate) unsafe fn unlock() {
    pac::FLASH.keyr().write(|w| w.set_fkeyr(0x4567_0123));
    pac::FLASH.keyr().write(|w| w.set_fkeyr(0xCDEF_89AB));
}

pub(crate) unsafe fn begin_write() {
    assert_eq!(0, WRITE_SIZE % 2);

    pac::FLASH.cr().write(|w| w.set_pg(true));
}

pub(crate) unsafe fn end_write() {
    pac::FLASH.cr().write(|w| w.set_pg(false));
}

pub(crate) unsafe fn blocking_write(start_address: u32, buf: &[u8; WRITE_SIZE]) -> Result<(), Error> {
    let mut address = start_address;
    for chunk in buf.chunks(2) {
        write_volatile(address as *mut u16, u16::from_le_bytes(chunk.try_into().unwrap()));
        address += chunk.len() as u32;

        // prevents parallelism errors
        fence(Ordering::SeqCst);
    }

    blocking_wait_ready()
}

pub(crate) unsafe fn blocking_erase_sector(sector: &FlashSector) -> Result<(), Error> {
    pac::FLASH.cr().modify(|w| {
        w.set_per(true);
    });

    pac::FLASH.ar().write(|w| w.set_far(sector.start));

    pac::FLASH.cr().modify(|w| {
        w.set_strt(true);
    });

    let mut ret: Result<(), Error> = blocking_wait_ready();

    if !pac::FLASH.sr().read().eop() {
        trace!("FLASH: EOP not set");
        ret = Err(Error::Prog);
    } else {
        pac::FLASH.sr().write(|w| w.set_eop(true));
    }

    pac::FLASH.cr().modify(|w| w.set_per(false));

    clear_all_err();
    if ret.is_err() {
        return ret;
    }
    Ok(())
}

pub(crate) unsafe fn clear_all_err() {
    pac::FLASH.sr().modify(|w| {
        if w.pgerr() {
            w.set_pgerr(true);
        }
        if w.wrprt() {
            w.set_wrprt(true)
        };
        if w.eop() {
            w.set_eop(true);
        }
    });
}

unsafe fn blocking_wait_ready() -> Result<(), Error> {
    loop {
        let sr = pac::FLASH.sr().read();

        if !sr.bsy() {
            if sr.wrprt() {
                return Err(Error::Protected);
            }

            if sr.pgerr() {
                return Err(Error::Seq);
            }

            return Ok(());
        }
    }
}
