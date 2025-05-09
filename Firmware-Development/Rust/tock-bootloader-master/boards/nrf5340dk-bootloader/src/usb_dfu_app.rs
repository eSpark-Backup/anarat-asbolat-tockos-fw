use kernel::hil;
use kernel::utilities::cells::TakeCell;
use kernel::hil::flash::Error;
use kernel::{create_capability, static_init};
use usbd_dfu::*;
use nrf_usbd::{UsbPeripheral, Usbd};
use usb_device::{bus::UsbBusAllocator, bus::UsbBus, prelude::*};
use usb_device::device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid, UsbDevice};
use kernel::deferred_call::{DeferredCall, DeferredCallClient};
use crate::bootloader_exit;
use core::mem::MaybeUninit;
use core::cell::RefCell;
use bootloader::flash_large_to_small::FiveTwelvePage;

static mut TEMP_BUFFER: [u8; 512] = [0u8; 512];
static mut PAGEBUFFER: MaybeUninit<bootloader::flash_large_to_small::FiveTwelvePage> = MaybeUninit::uninit();

pub struct MyMem<'a, F: hil::flash::Flash + 'static>  {
    buffer: [u8; 512],
    flash: &'a F,
    chunk_count: usize,
    nvmc: &'a nrf53::nvmc::Nvmc,
}

pub struct UsbDfuClass<'a, B: UsbBus, M: DFUMemIO> {
    pub usb_device: RefCell<UsbDevice<'a, B>>,
    pub dfu: RefCell<DFUClass<B, M>>,
    pub deferred_handle: DeferredCall,
}

impl<'a, B: UsbBus, M: DFUMemIO> DeferredCallClient for UsbDfuClass<'a, B, M> {
    fn handle_deferred_call(&self) {
        let mut usb_dev = self.usb_device.borrow_mut();
        let mut dfu = self.dfu.borrow_mut();
        usb_dev.poll(&mut [&mut *dfu]);
        self.deferred_handle.set();
    }

    fn register(&'static self) {
        self.deferred_handle.register(self);
    }
}

impl<'a, B: UsbBus, M: DFUMemIO> UsbDfuClass<'a, B, M> {
    pub fn new(
        usb_device: UsbDevice<'a, B>,
        dfu: DFUClass<B, M>,
        deferred_handle: DeferredCall,
    ) -> Self {
        UsbDfuClass {
            usb_device: RefCell::new(usb_device),
            dfu: RefCell::new(dfu),
            deferred_handle,
        }
    }
}

impl<'a, F: hil::flash::Flash + 'static> MyMem<'a, F> {
    pub fn new(buffer: [u8; 512], flash: &'a F, nvmc: &'a nrf53::nvmc::Nvmc) -> Self {
        Self {
            buffer,
            flash,
            chunk_count : 0,
            nvmc
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
    const PROGRAM_TIME_MS: u32 = 60;
    const ERASE_TIME_MS: u32 = 50;
    const FULL_ERASE_TIME_MS: u32 = 4000;
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
            self.flash.read_page(page_number, page_ref).map_err(|_| DFUMemError::Address)?;
            self.nvmc.handle_deferred_call();
            let bytes: &[u8] = page_ref_2.as_ref();
            TEMP_BUFFER[..length].copy_from_slice(&bytes[offset_within_page..end]);
            Ok(&TEMP_BUFFER[..length])
        }
    }
    

    fn erase(&mut self, address: u32) -> Result<(), DFUMemError> {
        // Erases a single page of 4KB.
        let page_index = address as usize / 512;
        self.flash.erase_page(page_index).map_err(|_| DFUMemError::Erase)?;
        self.nvmc.handle_deferred_call();
        Ok(())
    }

    fn erase_all(&mut self) -> Result<(), DFUMemError> {
        // Erase the flash from INITIAL_ADDRESS_POINTER to the last address of the flash.
        for addr in (Self::INITIAL_ADDRESS_POINTER as usize..0x100000).step_by(4096) {
            let page_index = addr / 512;
            self.flash.erase_page(page_index).map_err(|_| DFUMemError::Erase)?;
            self.nvmc.handle_deferred_call();
        }
        
        Ok(())
    }

    fn store_write_buffer(&mut self, src:&[u8]) -> Result<(), ()>{
        let offset = self.chunk_count * 128;
        self.buffer[offset..offset + src.len()].copy_from_slice(src);
        self.chunk_count += 1;
        Ok(())
    }

    fn program(&mut self, address: u32, length: usize) -> Result<(), DFUMemError> {
        if self.chunk_count == 4 {
            self.chunk_count = 0;
            // SAFETY: Assume PAGEBUFFER is properly initialized elsewhere
            let page_ref = unsafe { PAGEBUFFER.assume_init_mut() };
            let page_size = page_ref.as_mut().len();

            let page_num = (address as usize) / page_size;

            page_ref.as_mut()[..page_size].copy_from_slice(&self.buffer[..page_size]);

            // Write the full page back
            self.flash
                .write_page(page_num, page_ref)
                .map_err(|_| DFUMemError::Write)?;

        }
        Ok(())
    }
    

    fn manifestation(&mut self) -> Result<(), DFUManifestationError> {
        // Nothing to do to activate FW
        bootloader_exit();
        Ok(())
    }
}

pub fn usb_dfu_run<'a>(flash_adapter: &'static bootloader::flash_large_to_small::FlashLargeToSmall<'static, nrf5340::nvmc::Nvmc>, nvmc: &'static nrf53::nvmc::Nvmc) {
    unsafe {
        // Initialize the global PAGEBUFFER
        PAGEBUFFER.write(bootloader::flash_large_to_small::FiveTwelvePage::default());

        let my_mem_dfu = MyMem::new([0u8; 512], flash_adapter, nvmc);

        // Set up USB bus
        let usb_bus_alloc = static_init!(
            UsbBusAllocator<Usbd<Peripheral>>,
            UsbBusAllocator::new(Usbd::new(Peripheral))
        );

        // Create DFU class instance
        let mut dfu = DFUClass::new(&usb_bus_alloc, my_mem_dfu);

        let strings = StringDescriptors::default()
            .manufacturer("Ash")
            .product("Tock")
            .serial_number("5340");

        let usb_vid_pid = UsbVidPid(0x0483, 0xdf11);

        let usb_builder = UsbDeviceBuilder::new(usb_bus_alloc, usb_vid_pid)
            .strings(&[strings])
            .expect("Failed to set USB string descriptors")
            .device_release(0x0200)
            .self_powered(false)
            .max_power(250)
            .expect("Failed to set max_power")
            .max_packet_size_0(64)
            .expect("Failed to set max_packet_size_0");

        let mut usb_dev = usb_builder.build();

        let usb_dfu = static_init!(
            UsbDfuClass<Usbd<Peripheral>, MyMem<'static, bootloader::flash_large_to_small::FlashLargeToSmall<'static, nrf5340::nvmc::Nvmc>>>,
            UsbDfuClass {
                usb_device: RefCell::new(usb_dev),
                dfu: RefCell::new(dfu),
                deferred_handle: DeferredCall::new(),
            }
        );
        
        usb_dfu.register();         // Register with deferred call system
        usb_dfu.deferred_handle.set();  // Kick off the polling loop
    }
}

pub struct Peripheral;
unsafe impl UsbPeripheral for Peripheral {
    const REGISTERS: *const () = 0x50036000 as *const ();
}