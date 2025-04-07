// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2022.

//! Tock kernel for the Nordic Semiconductor nRF52840 development kit (DK).

#![no_std]
// Disable this attribute when documenting, as a workaround for
// https://github.com/rust-lang/rust/issues/62184.
#![cfg_attr(not(doc), no_main)]
#![deny(missing_docs)]

use core::ptr::addr_of_mut;
use core::ffi::c_void;

use kernel::debug;
use kernel::platform::{KernelResources, SyscallDriverLookup};
use kernel::{capabilities, create_capability};
use nrf5340dk_lib::{self, PROCESSES};

// State for loading and holding applications.
// How should the kernel respond when a process faults.
const FAULT_RESPONSE: capsules_system::process_policies::PanicFaultPolicy =
    capsules_system::process_policies::PanicFaultPolicy {};

struct Platform {
    base: nrf5340dk_lib::Platform,
    // eui64_driver: &'static nrf5340dk_lib::Eui64Driver,
    // ieee802154_driver: &'static nrf5340dk_lib::Ieee802154Driver,
    // udp_driver: &'static capsules_extra::net::udp::UDPDriver<'static>,
}

// pub const EXT_API_MAGIC: [u8; 4] = *b"FWIN";

/// Structure representing firmware metadata compatible with NSIB expectations.
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


/// Place the `FwInfo` struct at the correct offset using a custom linker section.
/// This must match the expected offset in NSIB (e.g., `.firmware_info`).
#[no_mangle]
#[used]
#[link_section = ".firmware_info"]
pub static FW_INFO: FwInfo = FwInfo {
    magic: [0x281ee6de, 0x8fcebb4c, 13570], // Equivalent to FIRMWARE_INFO_MAGIC
    total_size: core::mem::size_of::<FwInfo>() as u32,
    size: 0x40000,            // You can set this to your firmware image size
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


impl SyscallDriverLookup for Platform {
    fn with_driver<F, R>(&self, driver_num: usize, f: F) -> R
    where
        F: FnOnce(Option<&dyn kernel::syscall::SyscallDriver>) -> R,
    {
        match driver_num {
            // capsules_extra::eui64::DRIVER_NUM => f(Some(self.eui64_driver)),
            // capsules_extra::net::udp::DRIVER_NUM => f(Some(self.udp_driver)),
            // capsules_extra::ieee802154::DRIVER_NUM => f(Some(self.ieee802154_driver)),
            _ => self.base.with_driver(driver_num, f),
        }
    }
}

type Chip = nrf5340dk_lib::Chip;

impl KernelResources<Chip> for Platform {
    type SyscallDriverLookup = Self;
    type SyscallFilter = <nrf5340dk_lib::Platform as KernelResources<Chip>>::SyscallFilter;
    type ProcessFault = <nrf5340dk_lib::Platform as KernelResources<Chip>>::ProcessFault;
    type Scheduler = <nrf5340dk_lib::Platform as KernelResources<Chip>>::Scheduler;
    type SchedulerTimer = <nrf5340dk_lib::Platform as KernelResources<Chip>>::SchedulerTimer;
    type WatchDog = <nrf5340dk_lib::Platform as KernelResources<Chip>>::WatchDog;
    type ContextSwitchCallback =
        <nrf5340dk_lib::Platform as KernelResources<Chip>>::ContextSwitchCallback;

    fn syscall_driver_lookup(&self) -> &Self::SyscallDriverLookup {
        self
    }
    fn syscall_filter(&self) -> &Self::SyscallFilter {
        self.base.syscall_filter()
    }
    fn process_fault(&self) -> &Self::ProcessFault {
        self.base.process_fault()
    }
    fn scheduler(&self) -> &Self::Scheduler {
        self.base.scheduler()
    }
    fn scheduler_timer(&self) -> &Self::SchedulerTimer {
        self.base.scheduler_timer()
    }
    fn watchdog(&self) -> &Self::WatchDog {
        self.base.watchdog()
    }
    fn context_switch_callback(&self) -> &Self::ContextSwitchCallback {
        self.base.context_switch_callback()
    }
}

/// Main function called after RAM initialized.
#[no_mangle]
pub unsafe fn main() {
    let (board_kernel, base_platform, chip, default_peripherals, mux_alarm) =
        nrf5340dk_lib::start();

    //--------------------------------------------------------------------------
    // IEEE 802.15.4 and UDP
    //--------------------------------------------------------------------------

    // let (eui64_driver, ieee802154_driver, udp_driver) =
    //     nrf5340dk_lib::ieee802154_udp(board_kernel, default_peripherals, mux_alarm);

    let platform = Platform {
        base: base_platform,
        // eui64_driver,
        // ieee802154_driver,
        // udp_driver,
    };

    // These symbols are defined in the linker script.
    extern "C" {
        /// Beginning of the ROM region containing app images.
        static _sapps: u8;
        /// End of the ROM region containing app images.
        static _eapps: u8;
        /// Beginning of the RAM region for app memory.
        static mut _sappmem: u8;
        /// End of the RAM region for app memory.
        static _eappmem: u8;
    }

    debug!("owo\n");

    let process_management_capability =
        create_capability!(capabilities::ProcessManagementCapability);
    kernel::process::load_processes(
        board_kernel,
        chip,
        core::slice::from_raw_parts(
            core::ptr::addr_of!(_sapps),
            core::ptr::addr_of!(_eapps) as usize - core::ptr::addr_of!(_sapps) as usize,
        ),
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(_sappmem),
            core::ptr::addr_of!(_eappmem) as usize - core::ptr::addr_of!(_sappmem) as usize,
        ),
        &mut *addr_of_mut!(PROCESSES),
        &FAULT_RESPONSE,
        &process_management_capability,
    )
    .unwrap_or_else(|err| {
        debug!("Error loading processes!");
        debug!("{:?}", err);
    });

    let main_loop_capability = create_capability!(capabilities::MainLoopCapability);
    board_kernel.kernel_loop(
        &platform,
        chip,
        Some(&platform.base.ipc),
        &main_loop_capability,
    );
}
