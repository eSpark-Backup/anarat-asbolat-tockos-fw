//! Tock kernel for the bootloader on nrf52 over UART.
//!
//! It is based on nRF5340 SoC.

#![no_std]
// Disable this attribute when documenting, as a workaround for
// https://github.com/rust-lang/rust/issues/62184.
#![cfg_attr(not(doc), no_main)]

use core::panic::PanicInfo;

use kernel::capabilities;
use kernel::component::Component;
use kernel::create_capability;
use kernel::hil;
use kernel::hil::time::Alarm;
use kernel::hil::time::Counter;
use kernel::platform::KernelResources;
use kernel::platform::SyscallDriverLookup;
use kernel::static_init;
use kernel::hil::uart::Configure;
use kernel::hil::uart;
use kernel::utilities::cells::TakeCell;

use core::ops::Index;

use bootloader::null_scheduler::NullScheduler;

use capsules_core::virtualizers::virtual_alarm::VirtualMuxAlarm;

use nrf5340::gpio::Pin;
use nrf5340::interrupt_service::Nrf5340DefaultPeripherals;

use usb_device::{bus::UsbBusAllocator, prelude::*};
use usbd_dfu::*;
use core::mem::MaybeUninit;
use nrf_usbd::{UsbPeripheral, Usbd};
use usb_device::device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid};
use usbd_serial::{SerialPort, USB_CLASS_CDC};

const UART_TXD: Pin = Pin::P0_06;
const UART_RXD: Pin = Pin::P0_08;
const BUTTON_RST_PIN: Pin = Pin::P0_18;

const LED1_PIN: Pin = Pin::P0_28;
const LED2_PIN: Pin = Pin::P0_29;
const LED3_PIN: Pin = Pin::P0_30;
const LED4_PIN: Pin = Pin::P0_31;

#[allow(dead_code)]
const BUTTON_4: Pin = Pin::P0_25;

include!(concat!(env!("OUT_DIR"), "/attributes.rs"));

// Number of concurrent processes this platform supports.
const NUM_PROCS: usize = 0;

#[repr(C, packed)]
#[derive(Debug)]
pub struct FwInfo {
    /// Magic value to verify format compatibility.
    pub magic: [u32; 3], // MAGIC_LEN_WORDS = 3 (as per your earlier #define)

    /// Total size of this fw_info struct including the EXT_API lists.
    pub total_size: u32,

    /// Size of the firmware image code.
    pub size: u32,

    /// Monotonically increasing version counter.
    pub version: u32,

    /// Start address of the image.
    pub address: u32,

    /// Boot point (vector table) of the firmware.
    pub boot_address: u32,

    /// Validity flag (e.g., CONFIG_FW_INFO_VALID_VAL).
    pub valid: u32,

    /// Reserved (must be zero).
    pub reserved: [u32; 4],

    /// Number of EXT_APIs in ext_apis[] list.
    pub ext_api_num: u32,

    /// Number of EXT_API requests in ext_apis[] list.
    pub ext_api_request_num: u32,

    /// Flexible array of EXT_API entries follows this struct.
    pub ext_apis: [u8; 0], // Placeholder for flexible array (can also use `*const ExtApi`)
}

#[no_mangle]
#[used]
#[link_section = ".firmware_info"]
pub static FW_INFO: FwInfo = FwInfo {
    magic: [0x281ee6de, 0x8fcebb4c, 13570], // Equivalent to FIRMWARE_INFO_MAGIC
    total_size: core::mem::size_of::<FwInfo>() as u32,
    size: 0x4C00,            // You can set this to your firmware image size
    version: 1,
    address: 0x8000,         // Start address of firmware
    boot_address: 0x8000,    // Typically same as address, for vector table
    valid: 0x9102FFFF,                 // CONFIG_FW_INFO_VALID_VAL equivalent
    reserved: [0; 4],
    ext_api_num: 0,
    ext_api_request_num: 0,
    ext_apis: [],             // Flexible array; sized manually at runtime if needed
};
unsafe impl Sync for FwInfo {}

static mut PROCESSES: [Option<&'static dyn kernel::process::Process>; NUM_PROCS] =
    [None; NUM_PROCS];

static mut CHIP: Option<&'static nrf5340::chip::NRF53<Nrf5340DefaultPeripherals>> = None;

/// Dummy buffer that causes the linker to reserve enough space for the stack.
#[no_mangle]
#[link_section = ".stack_buffer"]
pub static mut STACK_MEMORY: [u8; 0x10000] = [0; 0x10000];

/// Function to allow the bootloader to exit by reseting the chip.
fn bootloader_exit() {
    unsafe {
        cortexm4::scb::reset();
    }
}

/// Supported drivers by the platform
pub struct Platform {
    bootloader: &'static bootloader::bootloader::Bootloader<
        'static,
        bootloader::uart_receive_multiple_timeout::UartReceiveMultipleTimeout<
            'static,
            VirtualMuxAlarm<'static, nrf53::rtc::Rtc<'static>>,
        >,
        bootloader::flash_large_to_small::FlashLargeToSmall<'static, nrf53::nvmc::Nvmc>,
    >,
    scheduler: &'static NullScheduler,
}

impl KernelResources<nrf5340::chip::NRF53<'static, Nrf5340DefaultPeripherals<'static>>>
    for Platform
{
    type SyscallDriverLookup = Self;
    type SyscallFilter = ();
    type ProcessFault = ();
    // type CredentialsCheckingPolicy = ();
    type Scheduler = NullScheduler;
    type SchedulerTimer = ();
    type WatchDog = ();
    type ContextSwitchCallback = ();

    fn syscall_driver_lookup(&self) -> &Self::SyscallDriverLookup {
        &self
    }
    fn syscall_filter(&self) -> &Self::SyscallFilter {
        &()
    }
    fn process_fault(&self) -> &Self::ProcessFault {
        &()
    }
    // fn credentials_checking_policy(&self) -> &'static Self::CredentialsCheckingPolicy {
    //     &()
    // }
    fn scheduler(&self) -> &Self::Scheduler {
        self.scheduler
    }
    fn scheduler_timer(&self) -> &Self::SchedulerTimer {
        &()
    }
    fn watchdog(&self) -> &Self::WatchDog {
        &()
    }
    fn context_switch_callback(&self) -> &Self::ContextSwitchCallback {
        &()
    }
}

impl SyscallDriverLookup for Platform {
    fn with_driver<F, R>(&self, _driver_num: usize, f: F) -> R
    where
        F: FnOnce(Option<&dyn kernel::syscall::SyscallDriver>) -> R,
    {
        f(None)
    }
}

#[inline(never)]
unsafe fn create_peripherals() -> &'static mut Nrf5340DefaultPeripherals<'static> {
    let ieee802154_ack_buf = static_init!(
        [u8; nrf5340::ieee802154_radio::ACK_BUF_SIZE],
        [0; nrf5340::ieee802154_radio::ACK_BUF_SIZE]
    );
    // Initialize chip peripheral drivers
    let nrf5340_peripherals = static_init!(
        Nrf5340DefaultPeripherals,
        Nrf5340DefaultPeripherals::new()
    );

    nrf5340_peripherals
}

pub struct MyMem<'a, F: hil::flash::Flash + 'static>  {
    buffer: [u8; 128],
    flash: &'a F,
    page_buffer: TakeCell<'static, F::Page>,
    temp_read_buffer: [u8; 128],
}

const temp_buffer: [u8; 128] = [0u8; 128];

impl<'a, F: hil::flash::Flash> DFUMemIO for MyMem<'a, F> {
    const MEM_INFO_STRING: &'static str = "@Flash/0x00018000/256*4Ka";
    const INITIAL_ADDRESS_POINTER: u32 = 0x0;
    const PROGRAM_TIME_MS: u32 = 8;
    const ERASE_TIME_MS: u32 = 50;
    const FULL_ERASE_TIME_MS: u32 = 50;
    const TRANSFER_SIZE: u16 = 128;

    fn read(&mut self, address: u32, length: usize) -> Result<&[u8], DFUMemError> {
        // TODO: check address value
        let offset = address as usize;
        let offset_bytes = (offset as u32).to_le_bytes(); // or to_be_bytes() for big-endian
        // self.temp_read_buffer[0..4].copy_from_slice(&offset_bytes);
        self.page_buffer.take().map(move |page| {
            // let pageB = page.index();
            let page_size = page.as_mut().len();
            let _ = self.flash.read_page(address as usize / page_size, page);
            // temp_buffer[..length].copy_from_slice(&page.as_ref()[..length]);


            // for i in 0..page_size {
            //     temp_buffer[i] = page.index()[i];
            // }
        });

        Ok(&temp_buffer[..length])
    }
    

    fn erase(&mut self, address: u32) -> Result<(), DFUMemError> {
        // TODO: check address value
        // let page_size = self.page_buffer.len();
        // let page_index = (address as usize) / page_size;
        let page_size = self.page_buffer.map_or(512, |page| page.as_mut().len());
        let _ = self.flash.erase_page(address as usize / page_size);
        // self.flash.erase_page(page_index)
        //     .map_err(|_| DFUMemError::Erase)
        // TODO: verify that block is erased successfully
        Ok(())
    }

    fn erase_all(&mut self) -> Result<(), DFUMemError> {
        // There is only one block, erase it.
        self.erase(0)
    }

    fn store_write_buffer(&mut self, src:&[u8]) -> Result<(), ()>{
        self.buffer[..src.len()].copy_from_slice(src);
        Ok(())
    }

    fn program(&mut self, address: u32, length: usize) -> Result<(), DFUMemError> {
  
        // Attempt to take the page buffer
        let result = self.page_buffer.take().map_or_else(
            || Err(DFUMemError::Write), // No page buffer available
            |page| {
                let page_size = page.as_mut().len();
    
                // if length > page_size {
                //     return Err(DFUMemError::Prog); // Too much data
                // }
    
                // Copy data from self.buffer to the page
                page.as_mut()[..length].copy_from_slice(&self.buffer[..length]);
    
                // Calculate page number
                let page_num = address as usize / page_size;
    
                // Attempt to write to flash
                let _ =  self.flash.write_page(page_num, page);
                Ok(())
            }
        );
    
        result
    }
    

    fn manifestation(&mut self) -> Result<(), DFUManifestationError> {
        // Nothing to do to activate FW
        bootloader_exit();
        Ok(())
    }
}

/// Entry point in the vector table called on hard reset.
#[no_mangle]
pub unsafe fn main() {
    // Loads relocations and clears BSS
    nrf5340::init();
    // Initialize chip peripheral drivers
    let nrf5340_peripherals = create_peripherals();

    // set up circular peripheral dependencies
    nrf5340_peripherals.init();
    let base_peripherals = &nrf5340_peripherals.nrf53;

    let board_kernel = static_init!(kernel::Kernel, kernel::Kernel::new(&PROCESSES));

    nrf53_components::startup::NrfStartupComponent::new(
        false,
        BUTTON_RST_PIN,
        nrf5340::uicr::Regulator0Output::DEFAULT,
        &base_peripherals.nvmc,
    ).finalize(());

    //--------------------------------------------------------------------------
    // BOOTLOADER ENTRY
    //--------------------------------------------------------------------------

    // Decide very early if we want to stay in the bootloader so we don't run a
    // bunch of init code just to reset into the kernel.
    let nrf_power = static_init!(
        nrf53::power::Power,
        nrf53::power::Power::new()
    );

    let bootloader_entry_mode = static_init!(
        bootloader_nrf53::bootloader_entry_gpregret::BootloaderEntryGpRegRet,
        bootloader_nrf53::bootloader_entry_gpregret::BootloaderEntryGpRegRet::new(nrf_power)
    );

    let bootloader_jumper = static_init!(
        bootloader_cortexm::jumper::CortexMJumper,
        bootloader_cortexm::jumper::CortexMJumper::new()
    );

    let active_notifier_led = static_init!(
        kernel::hil::led::LedLow<'static, nrf5340::gpio::GPIOPin>,
        kernel::hil::led::LedLow::new(&nrf5340_peripherals.gpio_port[LED1_PIN])
    );

    let bootloader_active_notifier = static_init!(
        bootloader::active_notifier_ledon::ActiveNotifierLedon,
        bootloader::active_notifier_ledon::ActiveNotifierLedon::new(active_notifier_led)
    );

    let bootloader_enterer = static_init!(
        bootloader::bootloader::BootloaderEnterer<'static>,
        bootloader::bootloader::BootloaderEnterer::new(
            bootloader_entry_mode,
            bootloader_jumper,
            bootloader_active_notifier
        )
    );

    // First decide if we want to actually run the bootloader or not.
    bootloader_enterer.check();

    //--------------------------------------------------------------------------
    // CAPABILITIES
    //--------------------------------------------------------------------------

    // Create capabilities that the board needs to call certain protected kernel
    // functions.
    let main_loop_capability = create_capability!(capabilities::MainLoopCapability);

    //--------------------------------------------------------------------------
    // ALARM & TIMER
    //--------------------------------------------------------------------------

    let rtc = &base_peripherals.rtc;
    let _ = rtc.start();

    let mux_alarm = components::alarm::AlarmMuxComponent::new(rtc)
        .finalize(components::alarm_mux_component_static!(nrf53::rtc::Rtc));

    // //--------------------------------------------------------------------------
    // // UART DEBUGGING
    // //--------------------------------------------------------------------------

    // let channel = nrf52_components::UartChannelComponent::new(
    //     UartChannel::Pins(UartPins::new(UART_RTS, UART_TXD, UART_CTS, UART_RXD)),
    //     mux_alarm,
    //     &base_peripherals.uarte0,
    // )
    // .finalize(());

    // // Create a shared UART channel for the console and for kernel debug.
    // let uart_mux =
    //     components::console::UartMuxComponent::new(channel, 115200, dynamic_deferred_caller)
    //         .finalize(());

    // // Create the debugger object that handles calls to `debug!()`.
    // components::debug_writer::DebugWriterComponent::new(uart_mux).finalize(());

    //--------------------------------------------------------------------------
    // SCHEDULER
    //--------------------------------------------------------------------------

    let null_scheduler = static_init!(NullScheduler, NullScheduler::new());

    // let usb_bus = UsbBusAllocator::new(Usbd::new(Peripheral));

    // let mut serial = SerialPort::new(&usb_bus);

    // let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
    //     .strings(&[
    //         StringDescriptors::new(usb_device::LangID::EN).product("nRF52840 Serial Port Demo")
    //     ])
    //     .unwrap()
    //     .device_class(USB_CLASS_CDC)
    //     .max_packet_size_0(64) // (makes control transfers 8x faster)
    //     .unwrap()
    //     .build();

    //--------------------------------------------------------------------------
    // BOOTLOADER
    //--------------------------------------------------------------------------

    // Setup receive with timeout.
    let recv_auto_virtual_alarm = static_init!(
        VirtualMuxAlarm<'static, nrf5340::rtc::Rtc>,
        VirtualMuxAlarm::new(mux_alarm)
    );
    recv_auto_virtual_alarm.setup();

    let recv_auto_uart = static_init!(
        bootloader::uart_receive_multiple_timeout::UartReceiveMultipleTimeout<
            'static,
            VirtualMuxAlarm<'static, nrf5340::rtc::Rtc>,
        >,
        bootloader::uart_receive_multiple_timeout::UartReceiveMultipleTimeout::new(
            &base_peripherals.uarte0,
            recv_auto_virtual_alarm,
            &mut bootloader::uart_receive_multiple_timeout::BUF
        )
    );
    recv_auto_virtual_alarm.set_alarm_client(recv_auto_uart);

    // Setup the UART pins
    let _ = base_peripherals.uarte0.initialize(
        nrf5340::pinmux::Pinmux::new(UART_TXD as u32),
        nrf5340::pinmux::Pinmux::new(UART_RXD as u32),
        None,
        None,
    );

    let nrfpagebuffer = static_init!(nrf53::nvmc::NrfPage, nrf53::nvmc::NrfPage::default());

    let flash_adapter = static_init!(
        bootloader::flash_large_to_small::FlashLargeToSmall<'static, nrf5340::nvmc::Nvmc>,
        bootloader::flash_large_to_small::FlashLargeToSmall::new(
            &base_peripherals.nvmc,
            nrfpagebuffer,
        )
    );
    hil::flash::HasClient::set_client(&base_peripherals.nvmc, flash_adapter);

    let pagebuffer = static_init!(
        bootloader::flash_large_to_small::FiveTwelvePage,
        bootloader::flash_large_to_small::FiveTwelvePage::default()
    );

    let bootloader = static_init!(
        bootloader::bootloader::Bootloader<
            'static,
            bootloader::uart_receive_multiple_timeout::UartReceiveMultipleTimeout<
                'static,
                VirtualMuxAlarm<'static, nrf5340::rtc::Rtc>,
            >,
            bootloader::flash_large_to_small::FlashLargeToSmall<'static, nrf53::nvmc::Nvmc>,
        >,
        bootloader::bootloader::Bootloader::new(
            recv_auto_uart,
            flash_adapter,
            &bootloader_exit,
            pagebuffer,
            &mut bootloader::bootloader::BUF
        )
    );

    // hil::uart::Transmit::set_transmit_client(&base_peripherals.uarte0, bootloader);
    // hil::uart::Receive::set_receive_client(&base_peripherals.uarte0, recv_auto_uart);
    // hil::uart::Receive::set_receive_client(recv_auto_uart, bootloader);
    hil::flash::HasClient::set_client(flash_adapter, bootloader);


    // Create USB device for a target device:
    let usbd = &nrf5340_peripherals.usbd;
    usbd.set_power_ref(nrf_power);
    
    let pagebuffer2 = static_init!(
        bootloader::flash_large_to_small::FiveTwelvePage,
        bootloader::flash_large_to_small::FiveTwelvePage::default()
    );

    let my_mem = MyMem {
        buffer: [0u8; 128],
        flash: flash_adapter,
        page_buffer: TakeCell::new(pagebuffer2),
        temp_read_buffer: [0u8; 128],
    };

    let usb_bus_alloc = UsbBusAllocator::new(Usbd::new(Peripheral));

    let mut dfu = DFUClass::new(&usb_bus_alloc, my_mem);

    let strings = StringDescriptors::default()
    .manufacturer("Ash")
    .product("Tock")
    .serial_number("5340");

    let usb_vid_pid = UsbVidPid(0x0483, 0xdf11);
    
    let usb_builder = UsbDeviceBuilder::new(&usb_bus_alloc, usb_vid_pid);
    
    let usb_builder = usb_builder
        .strings(&[strings])
        .expect("Failed to set USB string descriptors");

    let usb_builder = usb_builder
        .device_release(0x0200);

    let usb_builder = usb_builder
        .self_powered(false);

    let usb_builder = usb_builder
        .max_power(250)
        .expect("Failed to set max_power");

    let usb_builder = usb_builder
        .max_packet_size_0(64)
        .expect("Failed to set max_packet_size_0");

    let mut usb_dev = usb_builder
        .build();

    loop {
        if usb_dev.poll(&mut [&mut dfu]) {
            // DFUClass will handle the control transfers
        }
    }

    //--------------------------------------------------------------------------
    // FINAL SETUP AND BOARD BOOT
    //--------------------------------------------------------------------------

    // Start all of the clocks. Low power operation will require a better
    // approach than this.
    base_peripherals.clock.low_stop();
    base_peripherals.clock.high_stop();
    base_peripherals.clock.low_start();
    base_peripherals.clock.high_start();
    while !base_peripherals.clock.low_started() {}
    while !base_peripherals.clock.high_started() {}

    // let platform = Platform {
    //     bootloader,
    //     scheduler: null_scheduler,
    // };

    // let chip = static_init!(
    //     nrf5340::chip::NRF53<Nrf5340DefaultPeripherals>,
    //     nrf5340::chip::NRF53::new(nrf5340_peripherals)
    // );
    // CHIP = Some(chip);

    // // Actually run the bootloader.
    // platform.bootloader.start();

    // //--------------------------------------------------------------------------
    // // MAIN LOOP
    // //--------------------------------------------------------------------------

    // board_kernel.kernel_loop(
    //     &platform,
    //     chip,
    //     None::<&kernel::ipc::IPC<0>>,
    //     &main_loop_capability,
    // );
}

#[cfg(not(test))]
// #[no_mangle]
#[panic_handler]
unsafe fn panic_fmt(_pi: &PanicInfo) -> ! {
    loop {}
}

struct Peripheral;
unsafe impl UsbPeripheral for Peripheral {
    const REGISTERS: *const () = 0x50036000 as *const ();
}

