//! LaserCraft machine control, without any I/O of its own: the GRBL protocol ([`grbl`]), job
//! streaming, and a [`Controller`] that drives a machine through a [`Transport`] the caller
//! supplies (a serial port in the desktop app, [`sim::SimulatedGrbl`] in tests).
//!
//! GRBL's behaviour comes from its public interface documentation only; no GRBL source is used
//! (it is GPL), and the error and alarm meanings are written in our own words.

pub mod controller;
pub mod grbl;
pub mod sim;

pub use controller::{Command, Controller, Event};
pub use grbl::{Progress, State, Status};

/// A byte link to a controller: a serial port, a network socket, a simulator. Reads return what
/// has arrived (0 bytes when nothing has; a timeout may be reported as `TimedOut` or
/// `WouldBlock`, which mean the same).
pub trait Transport {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
}

/// A boxed link (a serial port or a simulator chosen at run time) is a link.
impl<T: Transport + ?Sized> Transport for Box<T> {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        (**self).write_all(bytes)
    }
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        (**self).read(buf)
    }
}
