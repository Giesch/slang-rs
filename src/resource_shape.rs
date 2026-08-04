//! Safe wrappers for the two bitfield-shaped reflection values.
//!
//! `slang.h` declares `SlangResourceShape` and `SlangBindingType` as bitfields:
//! a base shape/type in the low bits, plus independent flag bits, of which the
//! header names only a handful of products. The generated bindings model them
//! as `#[repr(transparent)]` newtypes that can hold any bit pattern, which is
//! correct but leaves the decoding to the caller. These wrappers do the
//! decoding: [`ResourceShape::base`] and [`BindingType::base`] pull out the
//! base value, and the predicates test the individual flags.
//!
//! The decode is total. A future Slang release that adds a base shape this
//! binding has no name for surfaces as [`BaseShape::Unrecognized`] rather than
//! anything worse, and the `Debug` impls print any bit outside the masks the
//! header defines.

use crate::sys;

/// Bit masks and flags, mirrored from the `SlangResourceShape` enumerators so
/// the decode below stays tied to the header.
mod shape_bits {
    use crate::sys::SlangResourceShape as S;

    pub const BASE_MASK: u32 = S::SlangResourceBaseShapeMask.0;

    pub const NONE: u32 = S::SlangResourceNone.0;
    pub const TEXTURE_1D: u32 = S::SlangTexture1d.0;
    pub const TEXTURE_2D: u32 = S::SlangTexture2d.0;
    pub const TEXTURE_3D: u32 = S::SlangTexture3d.0;
    pub const TEXTURE_CUBE: u32 = S::SlangTextureCube.0;
    pub const TEXTURE_BUFFER: u32 = S::SlangTextureBuffer.0;
    pub const STRUCTURED_BUFFER: u32 = S::SlangStructuredBuffer.0;
    pub const BYTE_ADDRESS_BUFFER: u32 = S::SlangByteAddressBuffer.0;
    pub const UNKNOWN: u32 = S::SlangResourceUnknown.0;
    pub const ACCELERATION_STRUCTURE: u32 = S::SlangAccelerationStructure.0;
    pub const TEXTURE_SUBPASS: u32 = S::SlangTextureSubpass.0;

    pub const FEEDBACK: u32 = S::SlangTextureFeedbackFlag.0;
    pub const SHADOW: u32 = S::SlangTextureShadowFlag.0;
    pub const ARRAY: u32 = S::SlangTextureArrayFlag.0;
    pub const MULTISAMPLE: u32 = S::SlangTextureMultisampleFlag.0;
    pub const COMBINED: u32 = S::SlangTextureCombinedFlag.0;

    /// Every bit this crate can name, base and flags together.
    pub const KNOWN_MASK: u32 = BASE_MASK | FEEDBACK | SHADOW | ARRAY | MULTISAMPLE | COMBINED;

    pub const FLAG_NAMES: [(u32, &str); 5] = [
        (FEEDBACK, "FEEDBACK"),
        (SHADOW, "SHADOW"),
        (ARRAY, "ARRAY"),
        (MULTISAMPLE, "MULTISAMPLE"),
        (COMBINED, "COMBINED"),
    ];
}

/// Bit masks and flags, mirrored from the `SlangBindingType` enumerators.
mod binding_bits {
    use crate::sys::SlangBindingType as B;

    pub const BASE_MASK: u32 = B::BaseMask.0;

    pub const UNKNOWN: u32 = B::Unknown.0;
    pub const SAMPLER: u32 = B::Sampler.0;
    pub const TEXTURE: u32 = B::Texture.0;
    pub const CONSTANT_BUFFER: u32 = B::ConstantBuffer.0;
    pub const PARAMETER_BLOCK: u32 = B::ParameterBlock.0;
    pub const TYPED_BUFFER: u32 = B::TypedBuffer.0;
    pub const RAW_BUFFER: u32 = B::RawBuffer.0;
    pub const COMBINED_TEXTURE_SAMPLER: u32 = B::CombinedTextureSampler.0;
    pub const INPUT_RENDER_TARGET: u32 = B::InputRenderTarget.0;
    pub const INLINE_UNIFORM_DATA: u32 = B::InlineUniformData.0;
    pub const RAY_TRACING_ACCELERATION_STRUCTURE: u32 = B::RayTracingAccelerationStructure.0;
    pub const VARYING_INPUT: u32 = B::VaryingInput.0;
    pub const VARYING_OUTPUT: u32 = B::VaryingOutput.0;
    pub const EXISTENTIAL_VALUE: u32 = B::ExistentialValue.0;
    pub const PUSH_CONSTANT: u32 = B::PushConstant.0;

    pub const MUTABLE: u32 = B::MutableFlag.0;

    /// Every bit this crate can name. Narrower than `BASE_MASK | EXT_MASK`:
    /// `MUTABLE` is the only extension bit the header gives a meaning.
    pub const KNOWN_MASK: u32 = BASE_MASK | MUTABLE;
}

/// The shape of a resource, with the flag bits stripped off.
///
/// [`BaseShape::Unknown`] is the header's own `SLANG_RESOURCE_UNKNOWN`, meaning
/// Slang could not classify the resource. [`BaseShape::Unrecognized`] means
/// this crate could not classify what Slang returned — see the type-level docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BaseShape {
    None,
    Texture1D,
    Texture2D,
    Texture3D,
    TextureCube,
    TextureBuffer,
    StructuredBuffer,
    ByteAddressBuffer,
    Unknown,
    AccelerationStructure,
    TextureSubpass,
    /// A base shape the pinned Slang produced and this crate has no name for,
    /// which means Slang gained a shape and [`BaseShape`] needs a variant.
    Unrecognized(u32),
}

impl BaseShape {
    fn from_bits(bits: u32) -> Self {
        use shape_bits as b;

        match bits {
            b::NONE => Self::None,
            b::TEXTURE_1D => Self::Texture1D,
            b::TEXTURE_2D => Self::Texture2D,
            b::TEXTURE_3D => Self::Texture3D,
            b::TEXTURE_CUBE => Self::TextureCube,
            b::TEXTURE_BUFFER => Self::TextureBuffer,
            b::STRUCTURED_BUFFER => Self::StructuredBuffer,
            b::BYTE_ADDRESS_BUFFER => Self::ByteAddressBuffer,
            b::UNKNOWN => Self::Unknown,
            b::ACCELERATION_STRUCTURE => Self::AccelerationStructure,
            b::TEXTURE_SUBPASS => Self::TextureSubpass,
            other => Self::Unrecognized(other),
        }
    }
}

/// A resource shape: a [`BaseShape`] plus any of the texture flag bits.
///
/// ```no_run
/// # use shader_slang::{BaseShape, reflection::Type};
/// # fn example(ty: &Type) {
/// let shape = ty.resource_shape();
/// if shape.base() == BaseShape::Texture2D && shape.is_array() {
///     // a `Texture2DArray`
/// }
/// # }
/// ```
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ResourceShape(sys::SlangResourceShape);

impl ResourceShape {
    pub const fn from_raw(raw: sys::SlangResourceShape) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> sys::SlangResourceShape {
        self.0
    }

    pub fn base(self) -> BaseShape {
        BaseShape::from_bits(self.0.0 & shape_bits::BASE_MASK)
    }

    pub const fn is_feedback(self) -> bool {
        self.has(shape_bits::FEEDBACK)
    }

    pub const fn is_shadow(self) -> bool {
        self.has(shape_bits::SHADOW)
    }

    pub const fn is_array(self) -> bool {
        self.has(shape_bits::ARRAY)
    }

    pub const fn is_multisample(self) -> bool {
        self.has(shape_bits::MULTISAMPLE)
    }

    /// True for a combined texture-sampler, such as GLSL's `sampler2D`.
    pub const fn is_combined(self) -> bool {
        self.has(shape_bits::COMBINED)
    }

    const fn has(self, flag: u32) -> bool {
        self.0.0 & flag != 0
    }
}

impl std::fmt::Debug for ResourceShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.base())?;

        for (flag, name) in shape_bits::FLAG_NAMES {
            if self.has(flag) {
                write!(f, "|{name}")?;
            }
        }

        let unnamed = self.0.0 & !shape_bits::KNOWN_MASK;
        if unnamed != 0 {
            write!(f, "|UnnamedBits({unnamed:#x})")?;
        }

        Ok(())
    }
}

/// The type of a binding, with the mutable flag stripped off.
///
/// [`BaseBindingType::Unknown`] is the header's own
/// `SLANG_BINDING_TYPE_UNKNOWN`; [`BaseBindingType::Unrecognized`] means this
/// crate could not classify what Slang returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BaseBindingType {
    Unknown,
    Sampler,
    Texture,
    ConstantBuffer,
    ParameterBlock,
    TypedBuffer,
    RawBuffer,
    CombinedTextureSampler,
    InputRenderTarget,
    InlineUniformData,
    RayTracingAccelerationStructure,
    VaryingInput,
    VaryingOutput,
    ExistentialValue,
    PushConstant,
    /// A binding type the pinned Slang produced and this crate has no name for,
    /// which means Slang gained a type and [`BaseBindingType`] needs a variant.
    Unrecognized(u32),
}

impl BaseBindingType {
    fn from_bits(bits: u32) -> Self {
        use binding_bits as b;

        match bits {
            b::UNKNOWN => Self::Unknown,
            b::SAMPLER => Self::Sampler,
            b::TEXTURE => Self::Texture,
            b::CONSTANT_BUFFER => Self::ConstantBuffer,
            b::PARAMETER_BLOCK => Self::ParameterBlock,
            b::TYPED_BUFFER => Self::TypedBuffer,
            b::RAW_BUFFER => Self::RawBuffer,
            b::COMBINED_TEXTURE_SAMPLER => Self::CombinedTextureSampler,
            b::INPUT_RENDER_TARGET => Self::InputRenderTarget,
            b::INLINE_UNIFORM_DATA => Self::InlineUniformData,
            b::RAY_TRACING_ACCELERATION_STRUCTURE => Self::RayTracingAccelerationStructure,
            b::VARYING_INPUT => Self::VaryingInput,
            b::VARYING_OUTPUT => Self::VaryingOutput,
            b::EXISTENTIAL_VALUE => Self::ExistentialValue,
            b::PUSH_CONSTANT => Self::PushConstant,
            other => Self::Unrecognized(other),
        }
    }
}

/// A binding type: a [`BaseBindingType`] plus the mutable flag.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BindingType(sys::SlangBindingType);

impl BindingType {
    pub const fn from_raw(raw: sys::SlangBindingType) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> sys::SlangBindingType {
        self.0
    }

    pub fn base(self) -> BaseBindingType {
        BaseBindingType::from_bits(self.0.0 & binding_bits::BASE_MASK)
    }

    /// True for the writable form of a binding, such as `RWTexture2D`.
    pub const fn is_mutable(self) -> bool {
        self.0.0 & binding_bits::MUTABLE != 0
    }
}

impl std::fmt::Debug for BindingType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.base())?;

        if self.is_mutable() {
            write!(f, "|MUTABLE")?;
        }

        let unnamed = self.0.0 & !binding_bits::KNOWN_MASK;
        if unnamed != 0 {
            write!(f, "|UnnamedBits({unnamed:#x})")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(bits: u32) -> ResourceShape {
        ResourceShape::from_raw(sys::SlangResourceShape(bits))
    }

    fn binding(bits: u32) -> BindingType {
        BindingType::from_raw(sys::SlangBindingType(bits))
    }

    #[test]
    fn decodes_named_products() {
        let array = shape(sys::SlangResourceShape::SlangTexture2dArray.0);
        assert_eq!(array.base(), BaseShape::Texture2D);
        assert!(array.is_array());
        assert!(!array.is_multisample());

        let ms_array = shape(sys::SlangResourceShape::SlangTexture2dMultisampleArray.0);
        assert_eq!(ms_array.base(), BaseShape::Texture2D);
        assert!(ms_array.is_array());
        assert!(ms_array.is_multisample());
    }

    /// The bit pattern from FloatyMonkey/slang-rs#28, which had no matching
    /// enum discriminant before the bindings became bitfields.
    #[test]
    fn decodes_combined_texture_sampler() {
        let combined = shape(0x102);
        assert_eq!(combined.base(), BaseShape::Texture2D);
        assert!(combined.is_combined());
        assert!(!combined.is_shadow());
        assert_eq!(format!("{combined:?}"), "Texture2D|COMBINED");
    }

    #[test]
    fn unrecognized_base_shapes_are_visible() {
        let future = shape(0x0B);
        assert_eq!(future.base(), BaseShape::Unrecognized(0x0B));
        assert_eq!(format!("{future:?}"), "Unrecognized(11)");

        let future_flag = shape(0x202 | 0x40);
        assert_eq!(future_flag.base(), BaseShape::Texture2D);
        assert_eq!(
            format!("{future_flag:?}"),
            "Texture2D|ARRAY|UnnamedBits(0x200)"
        );
    }

    #[test]
    fn decodes_binding_types() {
        let mutable = binding(sys::SlangBindingType::MutableTypedBuffer.0);
        assert_eq!(mutable.base(), BaseBindingType::TypedBuffer);
        assert!(mutable.is_mutable());
        assert_eq!(format!("{mutable:?}"), "TypedBuffer|MUTABLE");

        let sampler = binding(sys::SlangBindingType::Sampler.0);
        assert_eq!(sampler.base(), BaseBindingType::Sampler);
        assert!(!sampler.is_mutable());

        let future = binding(0xFF);
        assert_eq!(future.base(), BaseBindingType::Unrecognized(0xFF));
    }
}
