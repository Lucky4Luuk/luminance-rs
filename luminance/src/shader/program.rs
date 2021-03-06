//! Shader programs related types and functions.
//!
//! A shader `Program` is an object representing several operations. It’s a streaming program that
//! will operate on vertices, vertex patches, primitives and/or fragments.
//!
//! > *Note: shader programs don’t have to run on all those objects; they can be ran only on
//! vertices and fragments, for instance*.
//!
//! Creating a shader program is very simple. You need shader `Stage`s representing each step of the
//! processing.
//!
//! You *have* to provide at least a vertex and a fragment stages. If you want tessellation
//! processing, you need to provide a tessellation control and tessellation evaluation stages. If
//! you want primitives processing, you need to add a geometry stage.
//!
//! In order to customize the behavior of your shader programs, you have access to *uniforms*. For
//! more details about them, see the documentation for the type `Uniform` and trait `Uniformable`.
//! When creating a new shader program, you have to provide code to declare its *uniform semantics*.
//!
//! The *uniform semantics* represent a mapping between the variables declared in your shader
//! sources and variables you have access in your host code in Rust. Typically, you declare your
//! variable – `Uniform` – in Rust as `const` and use the function `Uniform::sem` to get the
//! semantic associated with the string you pass in.
//!
//! > **Becareful: currently, uniforms are a bit messy as you have to provide a per-program unique
//! number when you use the `Uniform::new` method. Efforts will be done in that direction in later
//! releases.
//!
//! You can create a `Program` with its `new` associated function.

#[cfg(feature = "std")]
use std::ffi::CString;
#[cfg(feature = "std")]
use std::fmt;
#[cfg(feature = "std")]
use std::marker::PhantomData;
#[cfg(feature = "std")]
use std::ops::Deref;
#[cfg(feature = "std")]
use std::ptr::null_mut;

#[cfg(not(feature = "std"))]
use alloc::prelude::ToOwned;
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(not(feature = "std"))]
use core::fmt::{self, Write};
#[cfg(not(feature = "std"))]
use core::marker::PhantomData;
#[cfg(not(feature = "std"))]
use core::ops::Deref;
#[cfg(not(feature = "std"))]
use core::ptr::null_mut;

use crate::linear::{M22, M33, M44};
use crate::metagl::*;
use crate::shader::stage::{self, Stage, StageError};
use crate::vertex::Semantics;

/// A raw shader program.
///
/// This is a type-erased version of a `Program`.
#[derive(Debug)]
pub struct RawProgram {
  handle: GLuint,
}

impl RawProgram {
  /// Create a new program by attaching shader stages.
  fn new<'a, T, G>(
    tess: T,
    vertex: &Stage,
    geometry: G,
    fragment: &Stage,
  ) -> Result<Self, ProgramError>
  where
    T: Into<Option<(&'a Stage, &'a Stage)>>,
    G: Into<Option<&'a Stage>>,
  {
    unsafe {
      let handle = gl::CreateProgram();

      if let Some((tcs, tes)) = tess.into() {
        gl::AttachShader(handle, tcs.handle());
        gl::AttachShader(handle, tes.handle());
      }

      gl::AttachShader(handle, vertex.handle());

      if let Some(geometry) = geometry.into() {
        gl::AttachShader(handle, geometry.handle());
      }

      gl::AttachShader(handle, fragment.handle());

      let program = RawProgram { handle };
      program.link().map(move |_| program)
    }
  }

  /// Link a program.
  fn link(&self) -> Result<(), ProgramError> {
    let handle = self.handle;

    unsafe {
      gl::LinkProgram(handle);

      let mut linked: GLint = gl::FALSE.into();
      gl::GetProgramiv(handle, gl::LINK_STATUS, &mut linked);

      if linked == gl::TRUE.into() {
        Ok(())
      } else {
        let mut log_len: GLint = 0;
        gl::GetProgramiv(handle, gl::INFO_LOG_LENGTH, &mut log_len);

        let mut log: Vec<u8> = Vec::with_capacity(log_len as usize);
        gl::GetProgramInfoLog(handle, log_len, null_mut(), log.as_mut_ptr() as *mut GLchar);

        gl::DeleteProgram(handle);

        log.set_len(log_len as usize);

        Err(ProgramError::LinkFailed(String::from_utf8(log).unwrap()))
      }
    }
  }

  #[inline]
  pub fn handle(&self) -> GLuint {
    self.handle
  }
}

impl Drop for RawProgram {
  fn drop(&mut self) {
    unsafe { gl::DeleteProgram(self.handle) }
  }
}

/// A typed shader program.
///
/// Typed shader programs represent their inputs, outputs and environment (uniforms) directly in
/// their types. This is very interesting as it adds more static safety and enables such programs
/// to *“store”* information like the uniform interface and such.
pub struct Program<S, Out, Uni> {
  raw: RawProgram,
  uni_iface: Uni,
  _in: PhantomData<*const S>,
  _out: PhantomData<*const Out>,
}

impl<S, Out, Uni> Program<S, Out, Uni>
where
  S: Semantics,
{
  /// Create a new program by consuming `Stage`s.
  pub fn from_stages<'a, T, G>(
    tess: T,
    vertex: &Stage,
    geometry: G,
    fragment: &Stage,
  ) -> Result<BuiltProgram<S, Out, Uni>, ProgramError>
  where
    Uni: UniformInterface,
    T: Into<Option<(&'a Stage, &'a Stage)>>,
    G: Into<Option<&'a Stage>>,
  {
    Self::from_stages_env(tess, vertex, geometry, fragment, ())
  }

  /// Create a new program by consuming strings.
  pub fn from_strings<'a, T, G>(
    tess: T,
    vertex: &str,
    geometry: G,
    fragment: &str,
  ) -> Result<BuiltProgram<S, Out, Uni>, ProgramError>
  where
    Uni: UniformInterface,
    T: Into<Option<(&'a str, &'a str)>>,
    G: Into<Option<&'a str>>,
  {
    Self::from_strings_env(tess, vertex, geometry, fragment, ())
  }

  /// Create a new program by consuming `Stage`s and by looking up an environment.
  pub fn from_stages_env<'a, E, T, G>(
    tess: T,
    vertex: &Stage,
    geometry: G,
    fragment: &Stage,
    env: E,
  ) -> Result<BuiltProgram<S, Out, Uni>, ProgramError>
  where
    Uni: UniformInterface<E>,
    T: Into<Option<(&'a Stage, &'a Stage)>>,
    G: Into<Option<&'a Stage>>,
  {
    let raw = RawProgram::new(tess, vertex, geometry, fragment)?;

    let mut warnings = bind_vertex_attribs_locations::<S>(&raw);

    raw.link()?;

    let (uni_iface, uniform_warnings) = create_uniform_interface(&raw, env)?;
    warnings.extend(uniform_warnings.into_iter().map(ProgramWarning::Uniform));

    let program = Program {
      raw,
      uni_iface,
      _in: PhantomData,
      _out: PhantomData,
    };

    Ok(BuiltProgram { program, warnings })
  }

  /// Create a new program by consuming strings.
  pub fn from_strings_env<'a, E, T, G>(
    tess: T,
    vertex: &str,
    geometry: G,
    fragment: &str,
    env: E,
  ) -> Result<BuiltProgram<S, Out, Uni>, ProgramError>
  where
    Uni: UniformInterface<E>,
    T: Into<Option<(&'a str, &'a str)>>,
    G: Into<Option<&'a str>>,
  {
    let tess = match tess.into() {
      Some((tcs_str, tes_str)) => {
        let tcs = Stage::new(stage::Type::TessellationControlShader, tcs_str)
          .map_err(ProgramError::StageError)?;
        let tes = Stage::new(stage::Type::TessellationEvaluationShader, tes_str)
          .map_err(ProgramError::StageError)?;
        Some((tcs, tes))
      }
      None => None,
    };

    let gs = match geometry.into() {
      Some(gs_str) => {
        Some(Stage::new(stage::Type::GeometryShader, gs_str).map_err(ProgramError::StageError)?)
      }
      None => None,
    };

    let vs = Stage::new(stage::Type::VertexShader, vertex).map_err(ProgramError::StageError)?;
    let fs = Stage::new(stage::Type::FragmentShader, fragment).map_err(ProgramError::StageError)?;

    Self::from_stages_env(
      tess.as_ref().map(|&(ref tcs, ref tes)| (tcs, tes)),
      &vs,
      gs.as_ref(),
      &fs,
      env,
    )
  }

  /// Get the program interface associated with this program.
  pub(crate) fn interface(&self) -> ProgramInterface<Uni> {
    let raw_program = &self.raw;
    let uniform_interface = &self.uni_iface;

    ProgramInterface {
      raw_program,
      uniform_interface,
    }
  }

  /// Transform the program to adapt the uniform interface.
  ///
  /// This function will not re-allocate nor recreate the GPU data. It will try to change the
  /// uniform interface and if the new uniform interface is correctly generated, return the same
  /// shader program updated with the new uniform interface. If the generation of the new uniform
  /// interface fails, this function will return the program with the former uniform interface.
  pub fn adapt<Q>(self) -> Result<BuiltProgram<S, Out, Q>, AdaptationFailure<S, Out, Uni>>
  where
    Q: UniformInterface,
  {
    self.adapt_env(())
  }

  /// Transform the program to adapt the uniform interface by looking up an environment.
  ///
  /// This function will not re-allocate nor recreate the GPU data. It will try to change the
  /// uniform interface and if the new uniform interface is correctly generated, return the same
  /// shader program updated with the new uniform interface. If the generation of the new uniform
  /// interface fails, this function will return the program with the former uniform interface.
  pub fn adapt_env<Q, E>(
    self,
    env: E,
  ) -> Result<BuiltProgram<S, Out, Q>, AdaptationFailure<S, Out, Uni>>
  where
    Q: UniformInterface<E>,
  {
    // first, try to create the new uniform interface
    let new_uni_iface = create_uniform_interface(&self.raw, env);

    match new_uni_iface {
      Ok((uni_iface, warnings)) => {
        // if we have succeeded, return self with the new uniform interface
        let program = Program {
          raw: self.raw,
          uni_iface,
          _in: PhantomData,
          _out: PhantomData,
        };
        let warnings = warnings.into_iter().map(ProgramWarning::Uniform).collect();

        Ok(BuiltProgram { program, warnings })
      }

      Err(iface_err) => {
        // we couldn’t generate the new uniform interface; return the error(s) that occurred and the
        // the untouched former program
        let failure = AdaptationFailure {
          program: self,
          error: iface_err,
        };
        Err(failure)
      }
    }
  }

  /// A version of [`Program::adapt_env`] that doesn’t change the uniform interface type.
  ///
  /// This function might be needed for when you want to update the uniform interface but still
  /// enforce that the type must remain the same.
  pub fn readapt_env<E>(
    self,
    env: E,
  ) -> Result<BuiltProgram<S, Out, Uni>, AdaptationFailure<S, Out, Uni>>
  where
    Uni: UniformInterface<E>,
  {
    self.adapt_env(env)
  }
}

impl<S, Out, Uni> Deref for Program<S, Out, Uni> {
  type Target = RawProgram;

  fn deref(&self) -> &Self::Target {
    &self.raw
  }
}

/// A built program with potential warnings.
///
/// The sole purpose of this type is to be destructured when a program is built.
pub struct BuiltProgram<S, Out, Uni> {
  /// Built program.
  pub program: Program<S, Out, Uni>,
  /// Potential warnings.
  pub warnings: Vec<ProgramWarning>,
}

impl<S, Out, Uni> BuiltProgram<S, Out, Uni> {
  /// Get the program and ignore the warnings.
  pub fn ignore_warnings(self) -> Program<S, Out, Uni> {
    self.program
  }
}

/// A [`Program`] uniform adaptation that has failed.
pub struct AdaptationFailure<S, Out, Uni> {
  /// Program used before trying to adapt.
  pub program: Program<S, Out, Uni>,
  /// Program error that prevented to adapt.
  pub error: ProgramError,
}

impl<S, Out, Uni> AdaptationFailure<S, Out, Uni> {
  /// Get the program and ignore the error.
  pub fn ignore_error(self) -> Program<S, Out, Uni> {
    self.program
  }
}

/// Class of types that can act as uniform interfaces in typed programs.
///
/// A uniform interface is a value that contains uniforms. The purpose of a uniform interface is to
/// be stored in a typed program and handed back when the program is made available in a pipeline.
///
/// The `E` type variable represents the environment and might be used to drive the implementation
/// from a value. It’s defaulted to `()` so that if you don’t use the environment, you don’t have to
/// worry about that value when creating the shader program.
pub trait UniformInterface<E = ()>: Sized {
  /// Build the uniform interface.
  ///
  /// When mapping a uniform, if you want to accept failures, you can discard the error and use
  /// `UniformBuilder::unbound` to let the uniform pass through, and collect the uniform warning.
  fn uniform_interface<'a>(builder: &mut UniformBuilder<'a>, env: E) -> Result<Self, ProgramError>;
}

impl UniformInterface for () {
  fn uniform_interface<'a>(_: &mut UniformBuilder<'a>, _: ()) -> Result<Self, ProgramError> {
    Ok(())
  }
}

/// Build uniforms to fold them to a uniform interface.
pub struct UniformBuilder<'a> {
  raw: &'a RawProgram,
  warnings: Vec<UniformWarning>,
}

impl<'a> UniformBuilder<'a> {
  fn new(raw: &'a RawProgram) -> Self {
    UniformBuilder {
      raw,
      warnings: Vec::new(),
    }
  }

  /// Have the builder hand you a `Uniform` of the type of your choice.
  ///
  /// Keep in mind that it’s possible that this function fails if you ask for a type for which the
  /// one defined in the shader doesn’t type match. If you don’t want a failure but an *unbound*
  /// uniform, head over to the `ask_unbound` function.
  pub fn ask<T>(&self, name: &str) -> Result<Uniform<T>, UniformWarning>
  where
    T: Uniformable,
  {
    let uniform = match T::ty() {
      Type::BufferBinding => self.ask_uniform_block(name)?,
      _ => self.ask_uniform(name)?,
    };

    uniform_type_match(self.raw.handle, name, T::ty())?;

    Ok(uniform)
  }

  /// Get an unbound [`Uniform`].
  ///
  /// Unbound [`Uniform`]s are not any different from typical [`Uniform`]s but when resolving
  /// mapping in the _shader program_, if the [`Uniform`] is found inactive or doesn’t exist,
  /// instead of returning an error, this function will return an _unbound uniform_, which is a
  /// uniform that does nothing interesting.
  ///
  /// That function is useful if you don’t really care about silently sending values down a shader
  /// program and getting them ignored. It might be the case for optional uniforms, for instance.
  pub fn ask_unbound<T>(&mut self, name: &str) -> Uniform<T>
  where
    T: Uniformable,
  {
    match self.ask(name) {
      Ok(uniform) => uniform,
      Err(warning) => {
        self.warnings.push(warning);
        self.unbound()
      }
    }
  }

  fn ask_uniform<T>(&self, name: &str) -> Result<Uniform<T>, UniformWarning>
  where
    T: Uniformable,
  {
    let location = {
      #[cfg(feature = "std")]
      {
        let c_name = CString::new(name.as_bytes()).unwrap();
        unsafe { gl::GetUniformLocation(self.raw.handle, c_name.as_ptr() as *const GLchar) }
      }

      #[cfg(not(feature = "std"))]
      {
        unsafe {
          with_cstring(name, |c_name| {
            gl::GetUniformLocation(self.raw.handle, c_name)
          })
          .unwrap_or(-1)
        }
      }
    };

    if location < 0 {
      Err(UniformWarning::Inactive(name.to_owned()))
    } else {
      Ok(Uniform::new(self.raw.handle, location))
    }
  }

  fn ask_uniform_block<T>(&self, name: &str) -> Result<Uniform<T>, UniformWarning>
  where
    T: Uniformable,
  {
    let location = {
      #[cfg(feature = "std")]
      {
        let c_name = CString::new(name.as_bytes()).unwrap();
        unsafe { gl::GetUniformBlockIndex(self.raw.handle, c_name.as_ptr() as *const GLchar) }
      }

      #[cfg(not(feature = "std"))]
      {
        unsafe {
          with_cstring(name, |c_name| {
            gl::GetUniformBlockIndex(self.raw.handle, c_name)
          })
          .unwrap_or(gl::INVALID_INDEX)
        }
      }
    };

    if location == gl::INVALID_INDEX {
      Err(UniformWarning::Inactive(name.to_owned()))
    } else {
      Ok(Uniform::new(self.raw.handle, location as GLint))
    }
  }

  /// Special uniform that won’t do anything.
  ///
  /// Use that function when you need a uniform to complete a uniform interface but you’re sure you
  /// won’t use it.
  pub fn unbound<T>(&self) -> Uniform<T>
  where
    T: Uniformable,
  {
    Uniform::unbound(self.raw.handle)
  }
}

/// The shader program interface.
///
/// This struct gives you access to several capabilities, among them:
///
///   - The typed *uniform interface* you would have acquired earlier.
///   - Some functions to query more data dynamically.
pub struct ProgramInterface<'a, Uni> {
  raw_program: &'a RawProgram,
  uniform_interface: &'a Uni,
}

impl<'a, Uni> Deref for ProgramInterface<'a, Uni> {
  type Target = Uni;

  fn deref(&self) -> &Self::Target {
    self.uniform_interface
  }
}

impl<'a, Uni> ProgramInterface<'a, Uni> {
  /// Get a [`UniformBuilder`] in order to perform dynamic uniform lookup.
  pub fn query(&'a self) -> UniformBuilder<'a> {
    UniformBuilder::new(self.raw_program)
  }
}

/// Errors that a `Program` can generate.
#[derive(Debug)]
pub enum ProgramError {
  /// A shader stage failed to compile or validate its state.
  StageError(StageError),
  /// Program link failed. You can inspect the reason by looking at the contained `String`.
  LinkFailed(String),
  /// Some uniform configuration is ill-formed. It can be a problem of inactive uniform, mismatch
  /// type, etc. Check the `UniformWarning` type for more information.
  UniformWarning(UniformWarning),
  /// Some vertex attribute is ill-formed.
  VertexAttribWarning(VertexAttribWarning),
}

impl fmt::Display for ProgramError {
  fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
    match *self {
      ProgramError::StageError(ref e) => write!(f, "shader program has stage error: {}", e),

      ProgramError::LinkFailed(ref s) => write!(f, "shader program failed to link: {}", s),

      ProgramError::UniformWarning(ref e) => {
        write!(f, "shader program contains uniform warning(s): {}", e)
      }
      ProgramError::VertexAttribWarning(ref e) => write!(
        f,
        "shader program contains vertex attribute warning(s): {}",
        e
      ),
    }
  }
}

/// Program warnings, not necessarily considered blocking errors.
#[derive(Debug)]
pub enum ProgramWarning {
  /// Some uniform configuration is ill-formed. It can be a problem of inactive uniform, mismatch
  /// type, etc. Check the `UniformWarning` type for more information.
  Uniform(UniformWarning),
  /// Some vertex attribute is ill-formed.
  VertexAttrib(VertexAttribWarning),
}

impl fmt::Display for ProgramWarning {
  fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
    match *self {
      ProgramWarning::Uniform(ref e) => write!(f, "uniform warning: {}", e),
      ProgramWarning::VertexAttrib(ref e) => write!(f, "vertex attribute warning: {}", e),
    }
  }
}

/// Warnings related to uniform issues.
#[derive(Debug)]
pub enum UniformWarning {
  /// Inactive uniform (not in use / no participation to the final output in shaders).
  Inactive(String),
  /// Type mismatch between the static requested type (i.e. the `T` in [`Uniform<T>`] for instance)
  /// and the type that got reflected from the backend in the shaders.
  ///
  /// The first `String` is the name of the uniform; the second one gives the type mismatch.
  TypeMismatch(String, Type),
}

impl UniformWarning {
  /// Create an inactive uniform warning.
  pub fn inactive<N>(name: N) -> Self
  where
    N: Into<String>,
  {
    UniformWarning::Inactive(name.into())
  }

  /// Create a type mismatch.
  pub fn type_mismatch<N>(name: N, ty: Type) -> Self
  where
    N: Into<String>,
  {
    UniformWarning::TypeMismatch(name.into(), ty)
  }
}

impl fmt::Display for UniformWarning {
  fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
    match *self {
      UniformWarning::Inactive(ref s) => write!(f, "inactive {} uniform", s),

      UniformWarning::TypeMismatch(ref n, ref t) => {
        write!(f, "type mismatch for uniform {}: {}", n, t)
      }
    }
  }
}

/// Warnings related to vertex attributes issues.
#[derive(Debug)]
pub enum VertexAttribWarning {
  /// Inactive vertex attribute (not read).
  Inactive(String),
}

impl fmt::Display for VertexAttribWarning {
  fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
    match *self {
      VertexAttribWarning::Inactive(ref s) => write!(f, "inactive {} vertex attribute", s),
    }
  }
}

/// A contravariant shader uniform. `Uniform<T>` doesn’t hold any value. It’s more like a mapping
/// between the host code and the shader the uniform was retrieved from.
#[derive(Debug)]
pub struct Uniform<T> {
  program: GLuint,
  index: GLint,
  _t: PhantomData<*const T>,
}

impl<T> Uniform<T>
where
  T: Uniformable,
{
  fn new(program: GLuint, index: GLint) -> Self {
    Uniform {
      program,
      index,
      _t: PhantomData,
    }
  }

  fn unbound(program: GLuint) -> Self {
    Uniform {
      program,
      index: -1,
      _t: PhantomData,
    }
  }

  pub(crate) fn program(&self) -> GLuint {
    self.program
  }

  pub(crate) fn index(&self) -> GLint {
    self.index
  }

  /// Update the value pointed by this uniform.
  pub fn update(&self, x: T) {
    x.update(self);
  }
}

/// Type of a uniform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Type {
  // scalars
  /// 32-bit signed integer.
  Int,
  /// 32-bit unsigned integer.
  UInt,
  /// 32-bit floating-point number.
  Float,
  /// Boolean.
  Bool,

  // vectors
  /// 2D signed integral vector.
  IVec2,
  /// 3D signed integral vector.
  IVec3,
  /// 4D signed integral vector.
  IVec4,
  /// 2D unsigned integral vector.
  UIVec2,
  /// 3D unsigned integral vector.
  UIVec3,
  /// 4D unsigned integral vector.
  UIVec4,
  /// 2D floating-point vector.
  Vec2,
  /// 3D floating-point vector.
  Vec3,
  /// 4D floating-point vector.
  Vec4,
  /// 2D boolean vector.
  BVec2,
  /// 3D boolean vector.
  BVec3,
  /// 4D boolean vector.
  BVec4,

  // matrices
  /// 2×2 floating-point matrix.
  M22,
  /// 3×3 floating-point matrix.
  M33,
  /// 4×4 floating-point matrix.
  M44,

  // textures
  /// Signed integral 1D texture sampler.
  ISampler1D,
  /// Signed integral 2D texture sampler.
  ISampler2D,
  /// Signed integral 3D texture sampler.
  ISampler3D,
  /// Unsigned integral 1D texture sampler.
  UISampler1D,
  /// Unsigned integral 2D texture sampler.
  UISampler2D,
  /// Unsigned integral 3D texture sampler.
  UISampler3D,
  /// Floating-point 1D texture sampler.
  Sampler1D,
  /// Floating-point 2D texture sampler.
  Sampler2D,
  /// Floating-point 3D texture sampler.
  Sampler3D,
  /// Signed cubemap sampler.
  ICubemap,
  /// Unsigned cubemap sampler.
  UICubemap,
  /// Floating-point cubemap sampler.
  Cubemap,

  // buffer
  /// Buffer binding; used for UBOs.
  BufferBinding,
}

impl fmt::Display for Type {
  fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
    match *self {
      Type::Int => f.write_str("int"),
      Type::UInt => f.write_str("uint"),
      Type::Float => f.write_str("float"),
      Type::Bool => f.write_str("bool"),
      Type::IVec2 => f.write_str("ivec2"),
      Type::IVec3 => f.write_str("ivec3"),
      Type::IVec4 => f.write_str("ivec4"),
      Type::UIVec2 => f.write_str("uvec2"),
      Type::UIVec3 => f.write_str("uvec3"),
      Type::UIVec4 => f.write_str("uvec4"),
      Type::Vec2 => f.write_str("vec2"),
      Type::Vec3 => f.write_str("vec3"),
      Type::Vec4 => f.write_str("vec4"),
      Type::BVec2 => f.write_str("bvec2"),
      Type::BVec3 => f.write_str("bvec3"),
      Type::BVec4 => f.write_str("bvec4"),
      Type::M22 => f.write_str("mat2"),
      Type::M33 => f.write_str("mat3"),
      Type::M44 => f.write_str("mat4"),
      Type::ISampler1D => f.write_str("isampler1D"),
      Type::ISampler2D => f.write_str("isampler2D"),
      Type::ISampler3D => f.write_str("isampler3D"),
      Type::UISampler1D => f.write_str("uSampler1D"),
      Type::UISampler2D => f.write_str("uSampler2D"),
      Type::UISampler3D => f.write_str("uSampler3D"),
      Type::Sampler1D => f.write_str("sampler1D"),
      Type::Sampler2D => f.write_str("sampler2D"),
      Type::Sampler3D => f.write_str("sampler3D"),
      Type::ICubemap => f.write_str("isamplerCube"),
      Type::UICubemap => f.write_str("usamplerCube"),
      Type::Cubemap => f.write_str("samplerCube"),
      Type::BufferBinding => f.write_str("buffer binding"),
    }
  }
}

/// Types that can behave as `Uniform`.
pub unsafe trait Uniformable: Sized {
  /// Update the uniform with a new value.
  fn update(self, u: &Uniform<Self>);
  /// Retrieve the `Type` of the uniform.
  fn ty() -> Type;
}

unsafe impl Uniformable for i32 {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform1i(u.index, self) }
  }

  fn ty() -> Type {
    Type::Int
  }
}

unsafe impl Uniformable for [i32; 2] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform2iv(u.index, 1, &self as *const i32) }
  }

  fn ty() -> Type {
    Type::IVec2
  }
}

unsafe impl Uniformable for [i32; 3] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform3iv(u.index, 1, &self as *const i32) }
  }

  fn ty() -> Type {
    Type::IVec3
  }
}

unsafe impl Uniformable for [i32; 4] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform4iv(u.index, 1, &self as *const i32) }
  }

  fn ty() -> Type {
    Type::IVec4
  }
}

unsafe impl<'a> Uniformable for &'a [i32] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform1iv(u.index, self.len() as GLsizei, self.as_ptr()) }
  }

  fn ty() -> Type {
    Type::Int
  }
}

unsafe impl<'a> Uniformable for &'a [[i32; 2]] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform2iv(u.index, self.len() as GLsizei, self.as_ptr() as *const i32) }
  }

  fn ty() -> Type {
    Type::IVec2
  }
}

unsafe impl<'a> Uniformable for &'a [[i32; 3]] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform3iv(u.index, self.len() as GLsizei, self.as_ptr() as *const i32) }
  }

  fn ty() -> Type {
    Type::IVec3
  }
}

unsafe impl<'a> Uniformable for &'a [[i32; 4]] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform4iv(u.index, self.len() as GLsizei, self.as_ptr() as *const i32) }
  }

  fn ty() -> Type {
    Type::IVec4
  }
}

unsafe impl Uniformable for u32 {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform1ui(u.index, self) }
  }

  fn ty() -> Type {
    Type::UInt
  }
}

unsafe impl Uniformable for [u32; 2] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform2uiv(u.index, 1, &self as *const u32) }
  }

  fn ty() -> Type {
    Type::UIVec2
  }
}

unsafe impl Uniformable for [u32; 3] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform3uiv(u.index, 1, &self as *const u32) }
  }

  fn ty() -> Type {
    Type::UIVec3
  }
}

unsafe impl Uniformable for [u32; 4] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform4uiv(u.index, 1, &self as *const u32) }
  }

  fn ty() -> Type {
    Type::UIVec4
  }
}

unsafe impl<'a> Uniformable for &'a [u32] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform1uiv(u.index, self.len() as GLsizei, self.as_ptr() as *const u32) }
  }

  fn ty() -> Type {
    Type::UInt
  }
}

unsafe impl<'a> Uniformable for &'a [[u32; 2]] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform2uiv(u.index, self.len() as GLsizei, self.as_ptr() as *const u32) }
  }

  fn ty() -> Type {
    Type::UIVec2
  }
}

unsafe impl<'a> Uniformable for &'a [[u32; 3]] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform3uiv(u.index, self.len() as GLsizei, self.as_ptr() as *const u32) }
  }

  fn ty() -> Type {
    Type::UIVec3
  }
}

unsafe impl<'a> Uniformable for &'a [[u32; 4]] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform4uiv(u.index, self.len() as GLsizei, self.as_ptr() as *const u32) }
  }

  fn ty() -> Type {
    Type::UIVec4
  }
}

unsafe impl Uniformable for f32 {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform1f(u.index, self) }
  }

  fn ty() -> Type {
    Type::Float
  }
}

unsafe impl Uniformable for [f32; 2] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform2fv(u.index, 1, &self as *const f32) }
  }

  fn ty() -> Type {
    Type::Vec2
  }
}

unsafe impl Uniformable for [f32; 3] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform3fv(u.index, 1, &self as *const f32) }
  }

  fn ty() -> Type {
    Type::Vec3
  }
}

unsafe impl Uniformable for [f32; 4] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform4fv(u.index, 1, &self as *const f32) }
  }

  fn ty() -> Type {
    Type::Vec4
  }
}

unsafe impl<'a> Uniformable for &'a [f32] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform1fv(u.index, self.len() as GLsizei, self.as_ptr() as *const f32) }
  }

  fn ty() -> Type {
    Type::Float
  }
}

unsafe impl<'a> Uniformable for &'a [[f32; 2]] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform2fv(u.index, self.len() as GLsizei, self.as_ptr() as *const f32) }
  }

  fn ty() -> Type {
    Type::Vec2
  }
}

unsafe impl<'a> Uniformable for &'a [[f32; 3]] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform3fv(u.index, self.len() as GLsizei, self.as_ptr() as *const f32) }
  }

  fn ty() -> Type {
    Type::Vec3
  }
}

unsafe impl<'a> Uniformable for &'a [[f32; 4]] {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform4fv(u.index, self.len() as GLsizei, self.as_ptr() as *const f32) }
  }

  fn ty() -> Type {
    Type::Vec4
  }
}

unsafe impl Uniformable for M22 {
  fn update(self, u: &Uniform<Self>) {
    let v = [self];
    unsafe { gl::UniformMatrix2fv(u.index, 1, gl::FALSE, v.as_ptr() as *const f32) }
  }

  fn ty() -> Type {
    Type::M22
  }
}

unsafe impl Uniformable for M33 {
  fn update(self, u: &Uniform<Self>) {
    let v = [self];
    unsafe { gl::UniformMatrix3fv(u.index, 1, gl::FALSE, v.as_ptr() as *const f32) }
  }

  fn ty() -> Type {
    Type::M33
  }
}

unsafe impl Uniformable for M44 {
  fn update(self, u: &Uniform<Self>) {
    let v = [self];
    unsafe { gl::UniformMatrix4fv(u.index, 1, gl::FALSE, v.as_ptr() as *const f32) }
  }

  fn ty() -> Type {
    Type::M44
  }
}

unsafe impl<'a> Uniformable for &'a [M22] {
  fn update(self, u: &Uniform<Self>) {
    unsafe {
      gl::UniformMatrix2fv(
        u.index,
        self.len() as GLsizei,
        gl::FALSE,
        self.as_ptr() as *const f32,
      )
    }
  }

  fn ty() -> Type {
    Type::M22
  }
}

unsafe impl<'a> Uniformable for &'a [M33] {
  fn update(self, u: &Uniform<Self>) {
    unsafe {
      gl::UniformMatrix3fv(
        u.index,
        self.len() as GLsizei,
        gl::FALSE,
        self.as_ptr() as *const f32,
      )
    }
  }

  fn ty() -> Type {
    Type::M33
  }
}

unsafe impl<'a> Uniformable for &'a [M44] {
  fn update(self, u: &Uniform<Self>) {
    unsafe {
      gl::UniformMatrix4fv(
        u.index,
        self.len() as GLsizei,
        gl::FALSE,
        self.as_ptr() as *const f32,
      )
    }
  }

  fn ty() -> Type {
    Type::M44
  }
}

unsafe impl Uniformable for bool {
  fn update(self, u: &Uniform<Self>) {
    unsafe { gl::Uniform1ui(u.index, self as GLuint) }
  }

  fn ty() -> Type {
    Type::Bool
  }
}

unsafe impl Uniformable for [bool; 2] {
  fn update(self, u: &Uniform<Self>) {
    let v = [self[0] as u32, self[1] as u32];
    unsafe { gl::Uniform2uiv(u.index, 1, &v as *const u32) }
  }

  fn ty() -> Type {
    Type::BVec2
  }
}

unsafe impl Uniformable for [bool; 3] {
  fn update(self, u: &Uniform<Self>) {
    let v = [self[0] as u32, self[1] as u32, self[2] as u32];
    unsafe { gl::Uniform3uiv(u.index, 1, &v as *const u32) }
  }

  fn ty() -> Type {
    Type::BVec3
  }
}

unsafe impl Uniformable for [bool; 4] {
  fn update(self, u: &Uniform<Self>) {
    let v = [
      self[0] as u32,
      self[1] as u32,
      self[2] as u32,
      self[3] as u32,
    ];
    unsafe { gl::Uniform4uiv(u.index, 1, &v as *const u32) }
  }

  fn ty() -> Type {
    Type::BVec4
  }
}

unsafe impl<'a> Uniformable for &'a [bool] {
  fn update(self, u: &Uniform<Self>) {
    let v: Vec<_> = self.iter().map(|x| *x as u32).collect();
    unsafe { gl::Uniform1uiv(u.index, v.len() as GLsizei, v.as_ptr()) }
  }

  fn ty() -> Type {
    Type::Bool
  }
}

unsafe impl<'a> Uniformable for &'a [[bool; 2]] {
  fn update(self, u: &Uniform<Self>) {
    let v: Vec<_> = self.iter().map(|x| [x[0] as u32, x[1] as u32]).collect();
    unsafe { gl::Uniform2uiv(u.index, v.len() as GLsizei, v.as_ptr() as *const u32) }
  }

  fn ty() -> Type {
    Type::BVec2
  }
}

unsafe impl<'a> Uniformable for &'a [[bool; 3]] {
  fn update(self, u: &Uniform<Self>) {
    let v: Vec<_> = self
      .iter()
      .map(|x| [x[0] as u32, x[1] as u32, x[2] as u32])
      .collect();
    unsafe { gl::Uniform3uiv(u.index, v.len() as GLsizei, v.as_ptr() as *const u32) }
  }

  fn ty() -> Type {
    Type::BVec3
  }
}

unsafe impl<'a> Uniformable for &'a [[bool; 4]] {
  fn update(self, u: &Uniform<Self>) {
    let v: Vec<_> = self
      .iter()
      .map(|x| [x[0] as u32, x[1] as u32, x[2] as u32, x[3] as u32])
      .collect();
    unsafe { gl::Uniform4uiv(u.index, v.len() as GLsizei, v.as_ptr() as *const u32) }
  }

  fn ty() -> Type {
    Type::BVec4
  }
}

// Check whether a shader program’s uniform type matches the type we have chosen.
fn uniform_type_match(program: GLuint, name: &str, ty: Type) -> Result<(), UniformWarning> {
  let mut size: GLint = 0;
  let mut glty: GLuint = 0;

  unsafe {
    // get the max length of the returned names
    let mut max_len = 0;
    gl::GetProgramiv(program, gl::ACTIVE_UNIFORM_MAX_LENGTH, &mut max_len);

    // get the index of the uniform
    let mut index = 0;

    #[cfg(feature = "std")]
    {
      let c_name = CString::new(name.as_bytes()).unwrap();
      gl::GetUniformIndices(
        program,
        1,
        [c_name.as_ptr() as *const GLchar].as_ptr(),
        &mut index,
      );
    }

    #[cfg(not(feature = "std"))]
    {
      let r = with_cstring(name, |c_name| {
        gl::GetUniformIndices(program, 1, [c_name].as_ptr(), &mut index);
      });

      if let Err(_) = r {
        #[cfg(feature = "std")]
        {
          return Err(format!("unable to find the index of {}", name));
        }

        #[cfg(not(feature = "std"))]
        {
          let mut reason = String::new();
          let _ = write!(&mut reason, "unable to find the index of {}", name);
          return Err(reason);
        }
      }
    }

    // get its size and type
    let mut name_ = Vec::<GLchar>::with_capacity(max_len as usize);
    gl::GetActiveUniform(
      program,
      index,
      max_len,
      null_mut(),
      &mut size,
      &mut glty,
      name_.as_mut_ptr(),
    );
  }

  // early-return if array – we don’t support them yet
  if size != 1 {
    return Ok(());
  }

  check_types_match(name, ty, glty)
}

// Check if a [`Type`] matches the OpenGL counterpart.
#[allow(clippy::cognitive_complexity)]
fn check_types_match(name: &str, ty: Type, glty: GLuint) -> Result<(), UniformWarning> {
  match ty {
    // scalars
    Type::Int if glty != gl::INT => Err(UniformWarning::type_mismatch(name, ty)),
    Type::UInt if glty != gl::UNSIGNED_INT => Err(UniformWarning::type_mismatch(name, ty)),
    Type::Float if glty != gl::FLOAT => Err(UniformWarning::type_mismatch(name, ty)),
    Type::Bool if glty != gl::BOOL => Err(UniformWarning::type_mismatch(name, ty)),
    // vectors
    Type::IVec2 if glty != gl::INT_VEC2 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::IVec3 if glty != gl::INT_VEC3 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::IVec4 if glty != gl::INT_VEC4 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::UIVec2 if glty != gl::UNSIGNED_INT_VEC2 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::UIVec3 if glty != gl::UNSIGNED_INT_VEC3 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::UIVec4 if glty != gl::UNSIGNED_INT_VEC4 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::Vec2 if glty != gl::FLOAT_VEC2 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::Vec3 if glty != gl::FLOAT_VEC3 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::Vec4 if glty != gl::FLOAT_VEC4 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::BVec2 if glty != gl::BOOL_VEC2 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::BVec3 if glty != gl::BOOL_VEC3 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::BVec4 if glty != gl::BOOL_VEC4 => Err(UniformWarning::type_mismatch(name, ty)),
    // matrices
    Type::M22 if glty != gl::FLOAT_MAT2 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::M33 if glty != gl::FLOAT_MAT3 => Err(UniformWarning::type_mismatch(name, ty)),
    Type::M44 if glty != gl::FLOAT_MAT4 => Err(UniformWarning::type_mismatch(name, ty)),
    // textures
    Type::ISampler1D if glty != gl::INT_SAMPLER_1D => Err(UniformWarning::type_mismatch(name, ty)),
    Type::ISampler2D if glty != gl::INT_SAMPLER_2D => Err(UniformWarning::type_mismatch(name, ty)),
    Type::ISampler3D if glty != gl::INT_SAMPLER_3D => Err(UniformWarning::type_mismatch(name, ty)),
    Type::UISampler1D if glty != gl::UNSIGNED_INT_SAMPLER_1D => {
      Err(UniformWarning::type_mismatch(name, ty))
    }
    Type::UISampler2D if glty != gl::UNSIGNED_INT_SAMPLER_2D => {
      Err(UniformWarning::type_mismatch(name, ty))
    }
    Type::UISampler3D if glty != gl::UNSIGNED_INT_SAMPLER_3D => {
      Err(UniformWarning::type_mismatch(name, ty))
    }
    Type::Sampler1D if glty != gl::SAMPLER_1D => Err(UniformWarning::type_mismatch(name, ty)),
    Type::Sampler2D if glty != gl::SAMPLER_2D => Err(UniformWarning::type_mismatch(name, ty)),
    Type::Sampler3D if glty != gl::SAMPLER_3D => Err(UniformWarning::type_mismatch(name, ty)),
    Type::ICubemap if glty != gl::INT_SAMPLER_CUBE => Err(UniformWarning::type_mismatch(name, ty)),
    Type::UICubemap if glty != gl::UNSIGNED_INT_SAMPLER_CUBE => {
      Err(UniformWarning::type_mismatch(name, ty))
    }
    Type::Cubemap if glty != gl::SAMPLER_CUBE => Err(UniformWarning::type_mismatch(name, ty)),
    _ => Ok(()),
  }
}

// Generate a uniform interface and collect warnings.
fn create_uniform_interface<Uni, E>(
  raw: &RawProgram,
  env: E,
) -> Result<(Uni, Vec<UniformWarning>), ProgramError>
where
  Uni: UniformInterface<E>,
{
  let mut builder = UniformBuilder::new(raw);
  let iface = Uni::uniform_interface(&mut builder, env)?;
  Ok((iface, builder.warnings))
}

fn bind_vertex_attribs_locations<S>(raw: &RawProgram) -> Vec<ProgramWarning>
where
  S: Semantics,
{
  let mut warnings = Vec::new();

  for desc in S::semantics_set() {
    match get_vertex_attrib_location(raw, &desc.name) {
      Ok(_) => {
        let index = desc.index as GLuint;

        // we are not interested in the location as we’re about to change it to what we’ve
        // decided in the semantics
        #[cfg(feature = "std")]
        {
          let c_name = CString::new(desc.name.as_bytes()).unwrap();
          unsafe { gl::BindAttribLocation(raw.handle, index, c_name.as_ptr() as *const GLchar) };
        }

        #[cfg(not(feature = "std"))]
        {
          unsafe {
            with_cstring(fmt.name, |c_name| {
              gl::BindAttribLocation(raw.handle, index, c_name.as_ptr() as *const GLchar);
            });
          }
        }
      }

      Err(warning) => warnings.push(ProgramWarning::VertexAttrib(warning)),
    }
  }

  warnings
}

fn get_vertex_attrib_location(raw: &RawProgram, name: &str) -> Result<GLuint, VertexAttribWarning> {
  let location = {
    #[cfg(feature = "std")]
    {
      let c_name = CString::new(name.as_bytes()).unwrap();
      unsafe { gl::GetAttribLocation(raw.handle, c_name.as_ptr() as *const GLchar) }
    }

    #[cfg(not(feature = "std"))]
    {
      unsafe {
        with_cstring(name, |c_name| gl::GetAttribLocation(raw.handle, c_name)).unwrap_or(-1)
      }
    }
  };

  if location < 0 {
    Err(VertexAttribWarning::Inactive(name.to_owned()))
  } else {
    Ok(location as _)
  }
}
