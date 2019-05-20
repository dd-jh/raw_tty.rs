// //! Managing raw mode.
// //!
// //! Raw mode is a particular state a TTY can have. It signifies that:
// //!
// //! 1. No line buffering (the input is given byte-by-byte).
// //! 2. The input is not written out, instead it has to be done manually by the programmer.
// //! 3. The output is not canonicalized (for example, `\n` means "go one line down", not "line
// //!    break").
// //!
// //! It is essential to design terminal programs.
// //!
// //! # Example
// //!
// //! ```rust,no_run
// //! use termion::raw::IntoRawMode;
// //! use std::io::{Write, stdout};
// //!
// //! fn main() {
// //!     let mut stdout = stdout().into_raw_mode().unwrap();
// //!
// //!     write!(stdout, "Hey there.").unwrap();
// //! }
// //! ```

mod util {
    use std::io;

    pub trait IsMinusOne {
        fn is_minus_one(&self) -> bool;
    }

    macro_rules! impl_is_minus_one {
            ($($t:ident)*) => ($(impl IsMinusOne for $t {
                fn is_minus_one(&self) -> bool {
                    *self == -1
                }
            })*)
        }

    impl_is_minus_one! { i8 i16 i32 i64 isize }

    pub fn convert_to_result<T: IsMinusOne>(t: T) -> io::Result<T> {
        if t.is_minus_one() {
            Err(io::Error::last_os_error())
        } else {
            Ok(t)
        }
    }
}

mod attr {
    #[cfg(unix)]
    pub mod unix {
        use crate::util::*;

        use libc::c_int;
        pub use libc::termios as Termios;
        use std::os::unix::io::RawFd;
        use std::{io, mem};

        pub fn get_terminal_attr(fd: RawFd) -> io::Result<Termios> {
            extern "C" {
                pub fn tcgetattr(fd: c_int, termptr: *mut Termios) -> c_int;
            }
            unsafe {
                let mut termios = mem::zeroed();
                convert_to_result(tcgetattr(fd, &mut termios))?;
                Ok(termios)
            }
        }

        pub fn set_terminal_attr(fd: RawFd, termios: &Termios) -> io::Result<()> {
            extern "C" {
                pub fn tcsetattr(fd: c_int, opt: c_int, termptr: *const Termios) -> c_int;
            }
            convert_to_result(unsafe { tcsetattr(fd, 0, termios) }).and(Ok(()))
        }

        pub fn raw_terminal_attr(termios: &mut Termios) {
            extern "C" {
                pub fn cfmakeraw(termptr: *mut Termios);
            }
            unsafe { cfmakeraw(termios) }
        }
    }

    #[cfg(unix)]
    pub use unix::*;
}

use attr::{get_terminal_attr, raw_terminal_attr, set_terminal_attr, Termios};
use std::io;
use std::os::unix::io::{AsRawFd, RawFd};

/// A terminal restorer, which keeps the previous state of the terminal, and restores it, when
/// dropped.
///
/// Restoring will entirely bring back the old TTY state.
pub struct TtyModeGuard {
    ios: Termios,
    fd: RawFd,
}

impl Drop for TtyModeGuard {
    fn drop(&mut self) {
        set_terminal_attr(self.fd, &self.ios).unwrap();
    }
}

impl TtyModeGuard {
    pub fn new(fd: RawFd) -> io::Result<TtyModeGuard> {
        let ios = get_terminal_attr(fd)?;

        Ok(TtyModeGuard { ios, fd })
    }

    pub fn set_raw_mode(&mut self) -> io::Result<()> {
        let mut ios = self.ios;

        raw_terminal_attr(&mut ios);

        set_terminal_attr(self.fd, &ios)?;
        Ok(())
    }

    pub fn modify_mode<F>(&mut self, f: F) -> io::Result<()>
    where
        F: FnOnce(Termios) -> Termios,
    {
        let ios = f(self.ios);
        set_terminal_attr(self.fd, &ios)?;
        Ok(())
    }
}

///// Types which can be converted into "raw mode".
/////
//pub trait RawMode: AsRawFd + Sized {
//    /// Switch to raw mode.
//    ///
//    /// Raw mode means that stdin won't be printed (it will instead have to be written manually by
//    /// the program). Furthermore, the input isn't canonicalised or buffered (that is, you can
//    /// read from stdin one byte of a time). The output is neither modified in any way.
//    fn raw_mode(&self) -> io::Result<TtyModeGuard>;
//}

//impl<T: AsRawFd> RawMode for T {
//    fn raw_mode(&self) -> io::Result<TtyModeGuard> {
//        Ok(self.save_tty_mode()?.with_raw_mode()?);
//        // let guard = self.save_tty_mode()?;
//        // let mut ios = guard.ios;

//        // raw_terminal_attr(&mut ios);

//        // set_terminal_attr(guard.fd, &ios)?;

//        // Ok(RawTerminal {
//        //     prev_ios: prev_ios,
//        //     output: self,
//        // })
//    }
//}

use std::io::Read;
use std::mem::ManuallyDrop;
use std::ops;

pub struct TtyWithGuard<T: AsRawFd> {
    inner: ManuallyDrop<T>,
    guard: ManuallyDrop<TtyModeGuard>,
}

impl<R: AsRawFd> ops::Deref for TtyWithGuard<R> {
    type Target = R;

    fn deref(&self) -> &R {
        &self.inner
    }
}

impl<R: AsRawFd> ops::DerefMut for TtyWithGuard<R> {
    fn deref_mut(&mut self) -> &mut R {
        &mut self.inner
    }
}

impl<R: AsRawFd> Drop for TtyWithGuard<R> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.guard);
            ManuallyDrop::drop(&mut self.inner);
        }
    }
}

impl<T: AsRawFd> TtyWithGuard<T> {
    pub fn new(tty: T) -> io::Result<TtyWithGuard<T>> {
        Ok(TtyWithGuard {
            guard: ManuallyDrop::new(TtyModeGuard::new(tty.as_raw_fd())?),
            inner: ManuallyDrop::new(tty),
        })
    }

    fn modify_mode<F>(&mut self, f: F) -> io::Result<()>
    where
        F: FnOnce(Termios) -> Termios,
    {
        self.guard.modify_mode(f)
    }

    pub fn set_raw_mode(&mut self) -> io::Result<()> {
        self.guard.set_raw_mode()
    }
}

pub trait GuardMode: AsRawFd + Sized {
    fn guard_mode(self) -> io::Result<TtyWithGuard<Self>>;
}

impl<T: AsRawFd> GuardMode for T {
    fn guard_mode(self) -> io::Result<TtyWithGuard<T>> {
        TtyWithGuard::new(self)
    }
}

pub struct RawReader<T: Read + AsRawFd>(TtyWithGuard<T>);

impl<R: Read + AsRawFd> Read for RawReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

/// Types which can be converted into "raw mode".
///
pub trait IntoRawMode: Read + AsRawFd + Sized {
    /// Switch to raw mode.
    ///
    /// Raw mode means that stdin won't be printed (it will instead have to be written manually by
    /// the program). Furthermore, the input isn't canonicalised or buffered (that is, you can
    /// read from stdin one byte of a time). The output is neither modified in any way.
    fn into_raw_mode(self) -> io::Result<TtyWithGuard<Self>>;
}

impl<T: Read + AsRawFd> IntoRawMode for T {
    fn into_raw_mode(self) -> io::Result<TtyWithGuard<T>> {
        let mut x = TtyWithGuard::new(self)?;
        x.set_raw_mode()?;
        Ok(x)
    }
}

// impl<W: Write + AsRawFd> RawReader<W> {
//     pub fn suspend_raw_mode(&self) -> io::Result<()> {
//         set_terminal_attr(self.as_raw_fd(), &self.prev_ios)?;
//         Ok(())
//     }

//     pub fn activate_raw_mode(&self) -> io::Result<()> {
//         let mut ios = get_terminal_attr(self.as_raw_fd())?;
//         raw_terminal_attr(&mut ios);
//         set_terminal_attr(self.as_raw_fd(), &ios)?;
//         Ok(())
//     }
// }

#[cfg(test)]
mod test {
    use super::*;
    use std::io::{self, stdin, stdout, Write};
    use std::os::unix::io::AsRawFd;

    #[test]
    fn test_into_raw_mode() -> io::Result<()> {
        let mut stdin = stdin().guard_mode()?;
        stdin.set_raw_mode()?;
        let mut out = stdout();

        out.write_all(b"this is a test, muahhahahah\r\n")?;

        drop(out);
        Ok(())
    }
}
