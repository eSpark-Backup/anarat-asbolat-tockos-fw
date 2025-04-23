/* Licensed under the Apache License, Version 2.0 or the MIT License. */
/* SPDX-License-Identifier: Apache-2.0 OR MIT                         */
/* Copyright Tock Contributors 2023.                                  */

 /* Set the firmware info section at the desired offset */
 . = ORIGIN(rom) + 0x200;  /* This will be replaced by the value of CONFIG_FW_INFO_OFFSET during the build */
 _fw_info_start = .;         /* Symbol for the start of the firmware info section */

 /* Place the firmware info section */
 KEEP(*(SORT_BY_NAME(.firmware_info*)))  /* Ensure that all sections matching .firmware_info* are placed here */

 /* Calculate the size of the firmware info section */
 _fw_info_size = ABSOLUTE(. - _fw_info_start);  /* Calculate the size by subtracting start from current position */
dssfsdfsdfsd