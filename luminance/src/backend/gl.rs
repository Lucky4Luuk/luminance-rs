//! OpenGL backend.

pub mod buffer;
pub mod depth_test;
pub mod framebuffer;
pub mod pixel;
pub mod state;
pub mod tess;
pub mod texture;

use self::state::GLState;
use std::cell::RefCell;
use std::rc::Rc;

/// The OpenGL backend.
pub struct GL {
  pub(crate) state: Rc<RefCell<GLState>>,
}
