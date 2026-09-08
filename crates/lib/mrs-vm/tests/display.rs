//! The memory-mapped display and the notification hook a frontend hangs off.

use std::task::Poll;

use mrs_core::display::{DISPLAY_ORIGIN, Rgb555};
use mrs_core::{Instruction, MemoryAddress, Opcode};
use mrs_vm::io::{IoError, MarieVmIODevice};
use mrs_vm::states::{RunOutcome, StepOutcome};
use mrs_vm::{MarieVM, io::VecIo};

const fn addr(value: u16) -> MemoryAddress {
    MemoryAddress::new(value)
}

/// A device that records every pixel it is told about, and can rewind.
#[derive(Debug, Default)]
struct Recorder {
    inner: VecIo,
    /// `(index, pixel)` in the order reported.
    writes: Vec<(usize, Rgb555)>,
}

impl MarieVmIODevice for Recorder {
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        self.inner.poll_input()
    }

    fn output(&mut self, value: i16) -> Result<(), IoError> {
        self.inner.output(value)
    }

    fn unread_input(&mut self, value: i16) -> bool {
        self.inner.unread_input(value)
    }

    fn unwrite_output(&mut self, value: i16) -> bool {
        self.inner.unwrite_output(value)
    }

    fn display_write(&mut self, index: usize, pixel: Rgb555) {
        self.writes.push((index, pixel));
    }
}

/// Builds `Load src; Store dst; Halt` with a data word, and runs it.
fn store_program(destination: u16, value: i16) -> [i16; 4] {
    [
        Instruction::new(Opcode::Load, addr(3)).encode().value(),
        Instruction::new(Opcode::Store, addr(destination))
            .encode()
            .value(),
        Instruction::new(Opcode::Halt, addr(0)).encode().value(),
        value,
    ]
}

#[test]
fn the_display_reads_the_top_of_memory() {
    let mut vm = MarieVM::new(VecIo::default());
    // Pure red at the first pixel, pure blue at the last.
    vm.memory_mut().write(addr(0xF00), 0x7C00_u16 as i16);
    vm.memory_mut().write(addr(0xFFF), 0x001F);

    let display = vm.display();
    assert_eq!(display.pixel(0, 0).unwrap().to_rgb8(), [255, 0, 0]);
    assert_eq!(display.pixel(15, 15).unwrap().to_rgb8(), [0, 0, 255]);
    assert_eq!(display.at(0).unwrap().bits(), 0x7C00);
    assert_eq!(display.at(255).unwrap().bits(), 0x001F);
    assert_eq!(display.at(256), None, "past the end");
    assert!(!display.is_blank());
}

#[test]
fn a_fresh_machine_has_a_blank_display() {
    let vm = MarieVM::new(VecIo::default());
    assert!(vm.display().is_blank());
    assert_eq!(vm.display().to_rgb8().len(), 256 * 3);
    assert!(vm.display().to_rgb8().iter().all(|byte| *byte == 0));
}

#[test]
fn rows_are_read_in_the_order_marie_js_indexes_them() {
    let mut vm = MarieVM::new(VecIo::default());
    // `0xF00 + 16 * row + column`, so this is column 1 of row 2.
    vm.memory_mut().write(addr(0xF00 + 16 * 2 + 1), 0x7FFF);

    let display = vm.display();
    assert_eq!(display.pixel(1, 2).unwrap(), Rgb555::WHITE);
    assert_eq!(display.pixel(2, 1).unwrap(), Rgb555::BLACK);

    let rows: Vec<Vec<Rgb555>> = display.rows().map(|row| row.collect()).collect();
    assert_eq!(rows.len(), 16);
    assert_eq!(rows[2][1], Rgb555::WHITE);
}

#[test]
fn storing_into_the_display_notifies_the_device() {
    let mut vm = MarieVM::new(Recorder::default());
    vm.load_program(addr(0), &store_program(0xF00, 0x7C00_u16 as i16))
        .unwrap();

    let RunOutcome::Terminated(vm) = vm.boot().run() else {
        panic!("should halt");
    };
    assert_eq!(vm.io().writes.len(), 1);
    let (index, pixel) = vm.io().writes[0];
    assert_eq!(index, 0, "row-major index");
    assert_eq!(pixel.to_rgb8(), [255, 0, 0]);
    // And the display itself agrees.
    assert_eq!(vm.display().pixel(0, 0).unwrap(), pixel);
}

#[test]
fn storing_outside_the_display_notifies_nothing() {
    let mut vm = MarieVM::new(Recorder::default());
    // 0xEFF is the word just below the display.
    vm.load_program(addr(0), &store_program(0xEFF, 0x7C00_u16 as i16))
        .unwrap();

    let RunOutcome::Terminated(vm) = vm.boot().run() else {
        panic!("should halt");
    };
    assert!(vm.io().writes.is_empty(), "{:?}", vm.io().writes);
}

#[test]
fn a_write_that_changes_nothing_is_not_reported() {
    // A program redrawing an identical frame should not make a frontend repaint.
    let mut vm = MarieVM::new(Recorder::default());
    vm.memory_mut().write(addr(0xF00), 0x7C00_u16 as i16);
    vm.load_program(addr(0), &store_program(0xF00, 0x7C00_u16 as i16))
        .unwrap();

    let RunOutcome::Terminated(vm) = vm.boot().run() else {
        panic!("should halt");
    };
    assert!(vm.io().writes.is_empty(), "{:?}", vm.io().writes);
}

#[test]
fn stepping_back_over_a_write_restores_the_pixel_and_says_so() {
    // A frontend drawing from the hook must not be left showing a stale pixel when the
    // debugger rewinds.
    let mut vm = MarieVM::new(Recorder::default());
    vm.set_history_limit(1000);
    vm.load_program(addr(0), &store_program(0xF00, 0x7C00_u16 as i16))
        .unwrap();

    let mut stepping = vm.debug();
    // Load, then Store.
    for _ in 0..2 {
        let StepOutcome::Stepped(next) = stepping.step() else {
            panic!("should step");
        };
        stepping = next;
    }
    assert_eq!(stepping.display().pixel(0, 0).unwrap().bits(), 0x7C00);
    assert_eq!(stepping.io().writes.len(), 1);

    stepping.step_back().expect("undo the Store");
    assert_eq!(
        stepping.display().pixel(0, 0).unwrap(),
        Rgb555::BLACK,
        "the pixel is restored"
    );
    // The device was told about the restoration as well as the write.
    assert_eq!(stepping.io().writes.len(), 2);
    assert_eq!(stepping.io().writes[1], (0, Rgb555::BLACK));
}

#[test]
fn an_indirect_store_into_the_display_is_reported_too() {
    // `StoreI` writes through a pointer, so the address is only known at run time.
    let program = [
        Instruction::new(Opcode::Load, addr(4)).encode().value(),
        Instruction::new(Opcode::StoreI, addr(3)).encode().value(),
        Instruction::new(Opcode::Halt, addr(0)).encode().value(),
        0x0F01,            // pointer to the second pixel
        0x03E0_u16 as i16, // pure green
    ];
    let mut vm = MarieVM::new(Recorder::default());
    vm.load_program(addr(0), &program).unwrap();

    let RunOutcome::Terminated(vm) = vm.boot().run() else {
        panic!("should halt");
    };
    assert_eq!(vm.io().writes, vec![(1, Rgb555::from_bits(0x03E0))]);
    assert_eq!(vm.display().pixel(1, 0).unwrap().to_rgb8(), [0, 255, 0]);
}

#[test]
fn the_display_starts_at_the_documented_address() {
    assert_eq!(DISPLAY_ORIGIN.value(), 0xF00);
    let mut vm = MarieVM::new(VecIo::default());
    vm.memory_mut().write(DISPLAY_ORIGIN, 0x7FFF);
    assert_eq!(vm.display().at(0).unwrap(), Rgb555::WHITE);
}
