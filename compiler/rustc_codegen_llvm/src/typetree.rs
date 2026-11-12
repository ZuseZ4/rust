use std::ffi::{CString, c_char};

use rustc_abi as abi;
use rustc_ast::expand::typetree::{FncTree, Kind, TypeTree as RustTypeTree};
use rustc_codegen_ssa::traits::{BaseTypeCodegenMethods, ConstCodegenMethods};
use rustc_session::config::AutoDiff;

use crate::attributes;
use crate::context::{CodegenCx, FullCx};
use crate::llvm::{self, EnzymeWrapper, TypeKind, Value};

fn to_enzyme_typetree<'ll>(
    rust_typetree: &RustTypeTree,
    _data_layout: &str,
    llcx: llvm::Context<'ll>,
) -> (llvm::TypeTree, Vec<llvm::TypeTree>) {
    let mut enzyme_tt = llvm::TypeTree::new();
    let extra_ints = process_typetree_recursive(&mut enzyme_tt, &rust_typetree, &[], llcx);

    let mut int_vec = vec![];
    for _ in 0..extra_ints {
        let mut int_tt = llvm::TypeTree::new();
        int_tt.insert(&[0], llvm::CConcreteType::DT_Integer, llcx);
        int_vec.push(int_tt);
    }

    (enzyme_tt, int_vec)
}

fn process_typetree_recursive<'ll>(
    enzyme_tt: &mut llvm::TypeTree,
    rust_typetree: &RustTypeTree,
    parent_indices: &[i64],
    llcx: llvm::Context<'ll>,
) -> u32 {
    let mut extra_ints = 0;
    for rust_type in &rust_typetree.0 {
        let concrete_type = match rust_type.kind {
            Kind::Anything => llvm::CConcreteType::DT_Anything,
            Kind::Integer => llvm::CConcreteType::DT_Integer,
            Kind::Pointer => llvm::CConcreteType::DT_Pointer,
            Kind::RustSlice => llvm::CConcreteType::DT_Pointer,
            Kind::Half => llvm::CConcreteType::DT_Half,
            Kind::Float => llvm::CConcreteType::DT_Float,
            Kind::Double => llvm::CConcreteType::DT_Double,
            Kind::F128 => llvm::CConcreteType::DT_FP128,
            Kind::Unknown => llvm::CConcreteType::DT_Unknown,
        };

        let mut indices = parent_indices.to_vec();
        if !parent_indices.is_empty() {
            indices.push(rust_type.offset as i64);
        } else if rust_type.offset == -1 {
            indices.push(-1);
        } else {
            indices.push(rust_type.offset as i64);
        }

        enzyme_tt.insert(&indices, concrete_type, llcx);

        if matches!(rust_type.kind, Kind::RustSlice) {
            // We lower slices to `ptr,int`, so add the int here.
            extra_ints += 1;
        }

        if matches!(rust_type.kind, Kind::Pointer | Kind::RustSlice)
            && !rust_type.child.0.is_empty()
        {
            process_typetree_recursive(enzyme_tt, &rust_type.child, &indices, llcx);
        }
    }
    extra_ints
}

// Describes all the locations in which we know how to apply an Enzyme TypeTree.
enum TTLocation {
    Definition,
    Callsite,
}

pub(crate) fn add_tt<'tcx, 'll>(cx: &FullCx<'ll, 'tcx>, fn_def: Value<'ll>, tt: FncTree) {
    // TypeTree processing uses functions from Enzyme. This feature is not strictly necessary,
    // but skipping this function increases the chance that Enzyme fails to compile some code.

    let tcx = cx.tcx;
    if !tcx.sess.opts.unstable_opts.autodiff.contains(&rustc_session::config::AutoDiff::Enable) {
        return;
    }
    if tcx.sess.opts.unstable_opts.autodiff.contains(&rustc_session::config::AutoDiff::NoTT) {
        return;
    }

    let llmod = cx.llmod;
    let llcx = cx.llcx;
    let inputs = tt.args;
    let ret_tt: RustTypeTree = tt.ret;

    let llvm_data_layout: *const c_char = unsafe { llvm::LLVMGetDataLayoutStr(llmod) };
    let llvm_data_layout =
        std::str::from_utf8(unsafe { std::ffi::CStr::from_ptr(llvm_data_layout) }.to_bytes())
            .expect("got a non-UTF8 data-layout from LLVM");

    let attr_name = "enzyme_type";
    let c_attr_name = CString::new(attr_name).unwrap();

    let tt_location: TTLocation =
        if llvm::LLVMRustIsCall(fn_def) { TTLocation::Callsite } else { TTLocation::Definition };

    let mut offset = 0;
    for (i, input) in inputs.iter().enumerate() {
        let (enzyme_tt, extra_ints) = to_enzyme_typetree(&input, llvm_data_layout, llcx);

        // This scope is just a visual reminder that we *must* drop the enzyme_wrapper before
        // we drop any typetrees (mainly enzyme_tt and extra_ints). Drop calls can not accept
        // arguments like an enzyme_wrapper, so the typetree drop impl has to call get_instance
        // on the static enzyme instance, which is behind a Mutex. Therefore we'd deadlock if we
        // hold the enzyme_wrapper while dropping the typetrees.
        {
            let enzyme_wrapper = EnzymeWrapper::get_instance();
            let c_str = enzyme_wrapper.tree_to_cstr(enzyme_tt.inner);

            let attr = llvm::CreateAttrStringValueFromCStr(llcx, &c_attr_name, &c_str);
            let arg_pos = llvm::AttributePlace::Argument(i as u32 + offset);
            // FIXME(autodiff): We currently know that this is correct for all the cases in which we
            // call this function. But we should make it more robust for the future.
            match tt_location {
                TTLocation::Definition => {
                    attributes::apply_to_llfn(fn_def, arg_pos, &[attr]);
                }
                TTLocation::Callsite => {
                    attributes::apply_to_callsite(fn_def, arg_pos, &[attr]);
                }
            }
            enzyme_wrapper.tree_to_string_free(c_str.as_ptr());
            for v in &extra_ints {
                offset += 1;
                let c_str = enzyme_wrapper.tree_to_cstr(v.inner);
                let int_attr = llvm::CreateAttrStringValueFromCStr(llcx, &c_attr_name, &c_str);
                let arg_pos = llvm::AttributePlace::Argument(i as u32 + offset);
                match tt_location {
                    TTLocation::Definition => {
                        attributes::apply_to_llfn(fn_def, arg_pos, &[int_attr]);
                    }
                    TTLocation::Callsite => {
                        attributes::apply_to_callsite(fn_def, arg_pos, &[int_attr]);
                    }
                }
                enzyme_wrapper.tree_to_string_free(c_str.as_ptr());
            }
        }
    }
    // We will only fail this if Rust types got lowered to LLVM in a way that we didn't predict.
    // Error, so we can learn from our mistakes.
    if matches!(tt_location, TTLocation::Definition) {
        let expected = offset as usize + inputs.len();
        let actual = llvm::count_params(fn_def) as usize;
        if expected != actual {
            tcx.dcx().warn(format!(
                "autodiff type-tree failure. We expected {expected} LLVM argument(s), \
                 but the generated LLVM function has {actual} parameter(s)"
            ));
        }
    }

    // FIXME(autodiff): We should think more about what it means if a function returns a slice or
    // other fat ptrs.
    let (enzyme_tt, _extra_ints) = to_enzyme_typetree(&ret_tt, llvm_data_layout, llcx);
    if ret_tt != RustTypeTree::new() {
        let enzyme_wrapper = EnzymeWrapper::get_instance();
        let c_str = enzyme_wrapper.tree_to_cstr(enzyme_tt.inner);
        let ret_attr = llvm::CreateAttrStringValueFromCStr(llcx, &c_attr_name, &c_str);
        let arg_pos = llvm::AttributePlace::ReturnValue;
        match tt_location {
            TTLocation::Definition => {
                attributes::apply_to_llfn(fn_def, arg_pos, &[ret_attr]);
            }
            TTLocation::Callsite => {
                attributes::apply_to_callsite(fn_def, arg_pos, &[ret_attr]);
            }
        }
        enzyme_wrapper.tree_to_string_free(c_str.as_ptr());
    }
}

/// Enzyme's name (`ConcreteType::str()`) for a scalar that is loaded or stored as `llty`.
///
/// A load or a store is a typed copy, so the operation may carry a type even though the memory
/// it touches has none. `None` means the operation stays undescribed:
/// - a union scalar (`MaybeUninit<T>`) may hold any bytes,
/// - a one-byte integer is the type used for untyped byte copies,
/// - `llty` must agree with the scalar, since the place type and the value can differ.
fn scalar_concrete_type<'ll>(
    cx: &CodegenCx<'ll, '_>,
    scalar: abi::Scalar,
    llty: llvm::Type<'ll>,
) -> Option<&'static [u8]> {
    let autodiff = &cx.tcx.sess.opts.unstable_opts.autodiff;
    if !autodiff.contains(&AutoDiff::Enable) || autodiff.contains(&AutoDiff::NoTT) {
        return None;
    }
    let abi::Scalar::Initialized { value, .. } = scalar else {
        return None;
    };
    let kind = cx.type_kind(llty);
    match value {
        abi::Primitive::Float(abi::Float::F16) if kind == TypeKind::Half => Some(b"Float@half"),
        abi::Primitive::Float(abi::Float::F32) if kind == TypeKind::Float => Some(b"Float@float"),
        abi::Primitive::Float(abi::Float::F64) if kind == TypeKind::Double => Some(b"Float@double"),
        abi::Primitive::Float(abi::Float::F128) if kind == TypeKind::FP128 => Some(b"Float@fp128"),
        abi::Primitive::Int(int, _) if kind == TypeKind::Integer && int.size().bytes() > 1 => {
            Some(b"Integer")
        }
        abi::Primitive::Pointer(_) if kind == TypeKind::Pointer => Some(b"Pointer"),
        _ => None,
    }
}

/// Attach `!enzyme_type` to a scalar load. It describes the loaded value, in the nested
/// encoding of Enzyme's `TypeTree::toMD`.
pub(crate) fn add_scalar_load_tt<'ll>(
    cx: &CodegenCx<'ll, '_>,
    load: Value<'ll>,
    scalar: abi::Scalar,
) {
    let Some(name) = scalar_concrete_type(cx, scalar, cx.val_ty(load)) else {
        return;
    };
    let leaf = cx.md_node_in_context(&[cx.create_metadata(name)]);
    let any_offset = llvm::LLVMValueAsMetadata(cx.const_i32(-1));
    cx.set_metadata_node(
        load,
        cx.get_md_kind_id("enzyme_type"),
        &[cx.create_metadata(b"Unknown"), any_offset, leaf],
    );
}

/// Attach `!enzyme_truetype` to a scalar store.
///
/// A store has no result, so `!enzyme_type` (which describes an instruction's result) can not be
/// used. `!enzyme_truetype` describes the operation: a list of `(type, start offset)` runs.
pub(crate) fn add_scalar_store_tt<'ll>(
    cx: &CodegenCx<'ll, '_>,
    store: Value<'ll>,
    val: Value<'ll>,
    scalar: abi::Scalar,
) {
    let Some(name) = scalar_concrete_type(cx, scalar, cx.val_ty(val)) else {
        return;
    };
    let start = llvm::LLVMValueAsMetadata(cx.const_i64(0));
    cx.set_metadata_node(
        store,
        cx.get_md_kind_id("enzyme_truetype"),
        &[cx.create_metadata(name), start],
    );
}
