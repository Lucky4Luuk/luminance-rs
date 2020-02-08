use crate::backend::color_slot::ColorSlot;
use crate::backend::depth_slot::DepthSlot;
use crate::backend::framebuffer::{Framebuffer as FramebufferBackend, FramebufferError};
use crate::backend::texture::{Dimensionable, Layerable, Sampler};
use crate::context::GraphicsContext;

pub struct Framebuffer<B, L, D, CS, DS>
where
  B: ?Sized + FramebufferBackend<L, D>,
  L: Layerable,
  D: Dimensionable,
  CS: ColorSlot<B, L, D>,
  DS: DepthSlot<B, L, D>,
{
  pub(crate) repr: B::FramebufferRepr,
  color_slot: CS::ColorTextures,
  depth_slot: DS::DepthTexture,
}

impl<B, L, D, CS, DS> Drop for Framebuffer<B, L, D, CS, DS>
where
  B: ?Sized + FramebufferBackend<L, D>,
  L: Layerable,
  D: Dimensionable,
  CS: ColorSlot<B, L, D>,
  DS: DepthSlot<B, L, D>,
{
  fn drop(&mut self) {
    unsafe { B::destroy_framebuffer(&mut self.repr) }
  }
}

impl<B, L, D, CS, DS> Framebuffer<B, L, D, CS, DS>
where
  B: ?Sized + FramebufferBackend<L, D>,
  L: Layerable,
  D: Dimensionable,
  CS: ColorSlot<B, L, D>,
  DS: DepthSlot<B, L, D>,
{
  pub fn new<C>(
    ctx: &mut C,
    size: D::Size,
    mipmaps: usize,
    sampler: Sampler,
  ) -> Result<Self, FramebufferError>
  where
    C: GraphicsContext<Backend = B>,
  {
    unsafe {
      let mut repr = ctx
        .backend()
        .new_framebuffer::<CS, DS>(size, mipmaps, &sampler)?;
      let color_slot = CS::reify_color_textures(ctx, size, mipmaps, &sampler, &mut repr, 0)?;
      let depth_slot = DS::reify_depth_texture(ctx, size, mipmaps, &sampler, &mut repr)?;

      let repr = B::validate_framebuffer(repr)?;

      Ok(Framebuffer {
        repr,
        color_slot,
        depth_slot,
      })
    }
  }

  pub fn size(&self) -> D::Size {
    unsafe { B::framebuffer_size(&self.repr) }
  }

  pub fn color_slot(&self) -> &CS::ColorTextures {
    &self.color_slot
  }

  pub fn depth_slot(&self) -> &DS::DepthTexture {
    &self.depth_slot
  }
}
