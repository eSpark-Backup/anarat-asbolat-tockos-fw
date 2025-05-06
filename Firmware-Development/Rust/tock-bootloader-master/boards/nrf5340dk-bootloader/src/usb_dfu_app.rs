use kernel::hil;
use kernel::utilities::cells::TakeCell;
use kernel::hil::flash::Error;
use kernel::{create_capability, static_init};
use usbd_dfu::*;
use nrf_usbd::{UsbPeripheral, Usbd};
use usb_device::{bus::UsbBusAllocator, bus::UsbBus, prelude::*};
use usb_device::device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid, UsbDevice};
use kernel::deferred_call::DeferredCall;
use crate::bootloader_exit;
use core::mem::MaybeUninit;
use bootloader::flash_large_to_small::FiveTwelvePage;

static mut TEMP_BUFFER: [u8; 512] = [0u8; 512];
static mut PAGEBUFFER: MaybeUninit<bootloader::flash_large_to_small::FiveTwelvePage> = MaybeUninit::uninit();

pub struct MyMem<'a, F: hil::flash::Flash + 'static>  {
    buffer: [u8; 128],
    flash: &'a F,
}

pub struct UsbDfuClass<'a, B: UsbBus> {
    pub usb_device: UsbDevice<'a, B>,
    pub dfu: DfuRuntimeClass<'a, B>,
    pub deferred_handle: DeferredCall,
}

impl<'a, F: hil::flash::Flash + 'static> MyMem<'a, F> {
    pub fn new(buffer: [u8; 128], flash: &'a F) -> Self {
        Self {
            buffer,
            flash,
        }
    }
}

impl<'a, F: hil::flash::Flash> hil::flash::Client<F> for MyMem<'a, F> {
    fn read_complete(&self, page: &'static mut F::Page, result: Result<(), Error>) {
        // Put the page buffer back for future use
        // self.page_buffer.replace(page);

        match result {
            Ok(()) => {
                // You can handle post-read logic here
                // For example, read from the page and update `self.buffer`
            }
            Err(e) => {
                // Handle the error (log, retry, etc.)
            }
        }
    }

    fn write_complete(&self, page: &'static mut F::Page, result: Result<(), Error>) {
        // self.page_buffer.replace(page);

        match result {
            Ok(()) => {
                // Post-write handling if needed
            }
            Err(e) => {
                // Handle the write error
            }
        }
    }

    fn erase_complete(&self, result: Result<(), Error>) {
        match result {
            Ok(()) => {
                // Erase successful
            }
            Err(e) => {
                // Handle erase error
            }
        }
    }
}

impl<'a, F> DFUMemIO for MyMem<'a, F>
where
    F: hil::flash::Flash<Page = FiveTwelvePage>,
{
    const MEM_INFO_STRING: &'static str = "@Flash/0x00018000/256*4Kg";
    const INITIAL_ADDRESS_POINTER: u32 = 0x18000;
    const PROGRAM_TIME_MS: u32 = 50;
    const ERASE_TIME_MS: u32 = 50;
    const FULL_ERASE_TIME_MS: u32 = 50;
    const TRANSFER_SIZE: u16 = 128;

    fn read(&mut self, address: u32, length: usize) -> Result<&[u8], DFUMemError> {
        // TODO: check address value
        let offset = address as usize;
        let offset_within_page = offset % 512;
        let page_number = offset / 512;
        let end = offset_within_page + length;
        unsafe {
            let page_ref = PAGEBUFFER.assume_init_mut();
            let page_ref_2 = PAGEBUFFER.assume_init_mut();
            self.flash.read_page(page_number, page_ref);
        //     match self.flash.read_page(page_number, page_ref) {
        //         Ok(_) => {
                    let bytes: &[u8] = page_ref_2.as_ref();
                    TEMP_BUFFER[..length].copy_from_slice(&bytes[offset_within_page..end]);
                    TEMP_BUFFER[0] = b'A';
                    Ok(&TEMP_BUFFER[..length])
                // }
                // Err(_) => Err(DFUMemError::Address),
            // }
        // Ok(&TEMP_BUFFER[..length])
        }

    }
    

    fn erase(&mut self, address: u32) -> Result<(), DFUMemError> {
        // let page_size = self.page_buffer.map_or(512, |page| page.as_mut().len());
        // let page_index = (address as usize) / page_size;
        // let result = self.flash.erase_page(page_index).map_err(|_| DFUMemError::Erase);
        // result
        Ok(())
    }

    fn erase_all(&mut self) -> Result<(), DFUMemError> {
        // There is only one block, erase it.
        Err(DFUMemError::Unknown)
    }

    fn store_write_buffer(&mut self, src:&[u8]) -> Result<(), ()>{
        self.buffer[..src.len()].copy_from_slice(src);
        Ok(())
    }

    fn program(&mut self, address: u32, length: usize) -> Result<(), DFUMemError> {
        // Attempt to take the page buffer
        // let result = self.page_buffer.take().map_or_else(
        //     || Err(DFUMemError::Write), // No page buffer available
        //     |page| {
        //         let page_size = page.as_mut().len();

        //         if length > page_size {
        //             return Err(DFUMemError::Prog); // Too much data
        //         }
    
        //         // Copy data from self.buffer to the page
        //         page.as_mut()[..length].copy_from_slice(&self.buffer[..length]);
    
        //         // Calculate page number
        //         let page_num = address as usize / page_size;
    
        //         // Attempt to write to flash
        //         let result =  self.flash.write_page(page_num, page);
        //         result.map_or(Err(DFUMemError::Write), |buffer| {
        //             self.page_buffer.replace(buffer);
        //             Ok(())
        //         })
                
        //     }
        // );
    
        // result
        Ok(())
    }
    

    fn manifestation(&mut self) -> Result<(), DFUManifestationError> {
        // Nothing to do to activate FW
        bootloader_exit();
        Ok(())
    }
}

pub fn usb_dfu_run(flash_adapter: &bootloader::flash_large_to_small::FlashLargeToSmall<'static, nrf5340::nvmc::Nvmc>) {
    unsafe {
        // Initialize the global PAGEBUFFER
        PAGEBUFFER.write(bootloader::flash_large_to_small::FiveTwelvePage::default());

        let my_mem_dfu = MyMem::new([0u8; 128], flash_adapter);

        // Set up USB bus
        let usb_bus_alloc = UsbBusAllocator::new(Usbd::new(Peripheral));

        // Create DFU class instance
        let mut dfu = DFUClass::new(&usb_bus_alloc, my_mem_dfu);

        let strings = StringDescriptors::default()
            .manufacturer("Ash")
            .product("Tock")
            .serial_number("5340");

        let usb_vid_pid = UsbVidPid(0x0483, 0xdf11);

        let usb_builder = UsbDeviceBuilder::new(&usb_bus_alloc, usb_vid_pid)
            .strings(&[strings])
            .expect("Failed to set USB string descriptors")
            .device_release(0x0200)
            .self_powered(false)
            .max_power(250)
            .expect("Failed to set max_power")
            .max_packet_size_0(64)
            .expect("Failed to set max_packet_size_0");

        let mut usb_dev = usb_builder.build();

        loop {
            if usb_dev.poll(&mut [&mut dfu]) {
                // DFUClass will handle the control transfers
            }
        }
    }
}

pub struct Peripheral;
unsafe impl UsbPeripheral for Peripheral {
    const REGISTERS: *const () = 0x50036000 as *const ();
}