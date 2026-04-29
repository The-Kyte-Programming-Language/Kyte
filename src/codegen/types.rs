use std::collections::HashSet;

use inkwell::types::BasicMetadataTypeEnum;
use inkwell::values::{BasicValueEnum, PointerValue};
use inkwell::AddressSpace;

use super::Codegen;
use crate::ast::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn i64_type(&self) -> inkwell::types::IntType<'ctx> {
        self.context.i64_type()
    }

    pub(super) fn f64_type(&self) -> inkwell::types::FloatType<'ctx> {
        self.context.f64_type()
    }

    pub(super) fn bool_type(&self) -> inkwell::types::IntType<'ctx> {
        self.context.bool_type()
    }

    pub(super) fn ptr_type(&self) -> inkwell::types::PointerType<'ctx> {
        self.context.ptr_type(AddressSpace::default())
    }

    pub(super) fn ty_to_llvm(&self, ty: &Ty) -> BasicMetadataTypeEnum<'ctx> {
        match ty {
            Ty::Int | Ty::I64 => self.i64_type().into(),
            Ty::I8 => self.context.i8_type().into(),
            Ty::I16 => self.context.i16_type().into(),
            Ty::I32 => self.context.i32_type().into(),
            Ty::U8 => self.context.i8_type().into(),
            Ty::U16 => self.context.i16_type().into(),
            Ty::U32 => self.context.i32_type().into(),
            Ty::U64 => self.i64_type().into(),
            Ty::Float => self.f64_type().into(),
            Ty::Bool => self.bool_type().into(),
            Ty::String => self.ptr_type().into(),
            Ty::Array(_) => self.ptr_type().into(),
            Ty::Struct(name) => self.struct_types[name].into(),
            Ty::Enum(name) => self
                .enum_types
                .get(name)
                .map(|t| (*t).into())
                .unwrap_or(self.i64_type().into()),
            Ty::Auto | Ty::TypeParam(_) | Ty::Fn(_, _) => self.i64_type().into(), // fallback
        }
    }

    /// Ty → LLVM IntType (Int/I8..U64 전용)
    pub(super) fn ty_to_int_type(&self, ty: &Ty) -> inkwell::types::IntType<'ctx> {
        match ty {
            Ty::I8 | Ty::U8 => self.context.i8_type(),
            Ty::I16 | Ty::U16 => self.context.i16_type(),
            Ty::I32 | Ty::U32 => self.context.i32_type(),
            Ty::Int | Ty::I64 | Ty::U64 => self.i64_type(),
            Ty::Bool => self.bool_type(),
            Ty::Enum(_) | Ty::Auto | Ty::TypeParam(_) => self.i64_type(),
            _ => self.i64_type(),
        }
    }

    /// Ty → BasicTypeEnum (elem_llvm_type 대체)
    pub(super) fn ty_to_basic(&self, ty: &Ty) -> inkwell::types::BasicTypeEnum<'ctx> {
        match ty {
            Ty::I8 | Ty::U8 => self.context.i8_type().into(),
            Ty::I16 | Ty::U16 => self.context.i16_type().into(),
            Ty::I32 | Ty::U32 => self.context.i32_type().into(),
            Ty::Int | Ty::I64 | Ty::U64 => self.i64_type().into(),
            Ty::Float => self.f64_type().into(),
            Ty::Bool => self.bool_type().into(),
            Ty::String => self.ptr_type().into(),
            Ty::Array(_) => self.ptr_type().into(),
            Ty::Struct(name) => self.struct_types[name].into(),
            Ty::Enum(name) => self
                .enum_types
                .get(name)
                .map(|t| (*t).into())
                .unwrap_or(self.i64_type().into()),
            Ty::Auto | Ty::TypeParam(_) | Ty::Fn(_, _) => self.i64_type().into(), // fallback
        }
    }

    pub(super) fn is_signed(ty: &Ty) -> bool {
        matches!(ty, Ty::Int | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64)
    }

    pub(super) fn is_integer_ty(ty: &Ty) -> bool {
        matches!(
            ty,
            Ty::Int | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
        )
    }

    pub(super) fn build_alloca(&self, name: &str, ty: &Ty) -> PointerValue<'ctx> {
        let entry = self.current_fn.unwrap().get_first_basic_block().unwrap();
        let temp_builder = self.context.create_builder();
        match entry.get_first_instruction() {
            Some(inst) => temp_builder.position_before(&inst),
            None => temp_builder.position_at_end(entry),
        }
        let llvm_ty = self.ty_to_basic(ty);
        temp_builder.build_alloca(llvm_ty, name).unwrap()
    }

    pub(super) fn global_string_ptr(&mut self, value: &str, prefix: &str) -> PointerValue<'ctx> {
        if let Some(ptr) = self.string_globals.get(value).copied() {
            return ptr;
        }

        let name = format!("{}_{}", prefix, self.string_global_counter);
        self.string_global_counter += 1;
        let ptr = self
            .builder
            .build_global_string_ptr(value, &name)
            .unwrap()
            .as_pointer_value();
        self.string_globals.insert(value.to_string(), ptr);
        ptr
    }

    pub(super) fn type_size_bytes(&mut self, ty: &Ty) -> u64 {
        let mut visiting = HashSet::new();
        self.type_size_bytes_with_visiting(ty, &mut visiting)
    }

    pub(super) fn type_size_bytes_with_visiting(
        &mut self,
        ty: &Ty,
        visiting: &mut HashSet<String>,
    ) -> u64 {
        match ty {
            Ty::I8 | Ty::U8 | Ty::Bool => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 => 4,
            Ty::Int | Ty::I64 | Ty::U64 => 8,
            Ty::Float => 8,
            Ty::String | Ty::Array(_) => 8, // pointer size
            Ty::Struct(name) => self.struct_size_bytes(name, visiting),
            Ty::Enum(name) => {
                let payload = self.enum_payload_sizes.get(name).copied().unwrap_or(0);
                4 + payload // tag (i32) + payload
            }
            Ty::Auto | Ty::TypeParam(_) | Ty::Fn(_, _) => 8, // fallback
        }
    }

    pub(super) fn struct_size_bytes(&mut self, name: &str, visiting: &mut HashSet<String>) -> u64 {
        if let Some(size) = self.struct_size_cache.get(name).copied() {
            return size;
        }
        if !visiting.insert(name.to_string()) {
            return 8;
        }

        let size = if let Some(fields) = self.struct_defs.get(name).cloned() {
            let total = fields
                .iter()
                .map(|field| self.type_size_bytes_with_visiting(&field.ty, visiting))
                .sum::<u64>();
            total.max(1)
        } else {
            8
        };

        visiting.remove(name);
        self.struct_size_cache.insert(name.to_string(), size);
        size
    }

    pub(super) fn struct_field_info(&self, sname: &str, field: &str) -> Option<(u32, Ty)> {
        let fields = self.struct_defs.get(sname)?;
        for (idx, f) in fields.iter().enumerate() {
            if f.name == field {
                return Some((idx as u32, f.ty.clone()));
            }
        }
        None
    }

    pub(super) fn elem_llvm_type(&self, ty: &Ty) -> inkwell::types::BasicTypeEnum<'ctx> {
        self.ty_to_basic(ty)
    }

    /// IntLit은 항상 i64로 생성되므로, 대상 타입에 맞게 truncate/extend
    pub(super) fn coerce_to_ty(
        &self,
        val: BasicValueEnum<'ctx>,
        target: &Ty,
    ) -> BasicValueEnum<'ctx> {
        if !Self::is_integer_ty(target) || matches!(target, Ty::Int | Ty::I64 | Ty::U64) {
            return val;
        }
        if let BasicValueEnum::IntValue(iv) = val {
            let target_ty = self.ty_to_int_type(target);
            let src_width = iv.get_type().get_bit_width();
            let dst_width = target_ty.get_bit_width();
            if src_width == dst_width {
                return val;
            }
            if src_width > dst_width {
                return self
                    .builder
                    .build_int_truncate(iv, target_ty, "trunc")
                    .unwrap()
                    .into();
            } else if Self::is_signed(target) {
                return self
                    .builder
                    .build_int_s_extend(iv, target_ty, "sext")
                    .unwrap()
                    .into();
            } else {
                return self
                    .builder
                    .build_int_z_extend(iv, target_ty, "zext")
                    .unwrap()
                    .into();
            }
        }
        val
    }

    pub(super) fn no_terminator(&self) -> bool {
        self.builder
            .get_insert_block()
            .map(|bb| bb.get_terminator().is_none())
            .unwrap_or(true)
    }
}
