//@ compile-flags: -Zautodiff=Enable -C opt-level=0 -Clto=fat
//@ edition: 2024
//@ no-prefer-dynamic
//@ needs-enzyme

// Scalar loads and stores are typed copies, so they carry an Enzyme type: `!enzyme_type` on the
// load (it describes the loaded value) and `!enzyme_truetype` on the store (it has no result).
// One-byte integers and `MaybeUninit` scalars may hold any bytes and stay undescribed.

use std::mem::MaybeUninit;

// CHECK-LABEL: @copy_f64
// CHECK: load double, ptr %{{.*}}, !enzyme_type ![[F64_LOAD:[0-9]+]]
// CHECK: store double %{{.*}}, ptr %{{.*}}, !enzyme_truetype ![[F64_STORE:[0-9]+]]
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe fn copy_f64(dst: *mut f64, src: *const f64) {
    unsafe { *dst = *src }
}

// CHECK-LABEL: @copy_f32
// CHECK: load float, ptr %{{.*}}, !enzyme_type ![[F32_LOAD:[0-9]+]]
// CHECK: store float %{{.*}}, ptr %{{.*}}, !enzyme_truetype ![[F32_STORE:[0-9]+]]
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe fn copy_f32(dst: *mut f32, src: *const f32) {
    unsafe { *dst = *src }
}

// CHECK-LABEL: @copy_usize
// CHECK: load i64, ptr %{{.*}}, !enzyme_type ![[INT_LOAD:[0-9]+]]
// CHECK: store i64 %{{.*}}, ptr %{{.*}}, !enzyme_truetype ![[INT_STORE:[0-9]+]]
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe fn copy_usize(dst: *mut usize, src: *const usize) {
    unsafe { *dst = *src }
}

// CHECK-LABEL: @copy_ptr
// CHECK: load ptr, ptr %{{.*}}, !enzyme_type ![[PTR_LOAD:[0-9]+]]
// CHECK: store ptr %{{.*}}, ptr %{{.*}}, !enzyme_truetype ![[PTR_STORE:[0-9]+]]
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe fn copy_ptr(dst: *mut *const f64, src: *const *const f64) {
    unsafe { *dst = *src }
}

// CHECK-LABEL: @copy_slice_ref
// CHECK: load ptr, ptr %{{.*}}, !enzyme_type ![[PTR_LOAD]]
// CHECK: load i64, ptr %{{.*}}, !enzyme_type ![[INT_LOAD]]
// CHECK: store ptr %{{.*}}, ptr %{{.*}}, !enzyme_truetype ![[PTR_STORE]]
// CHECK: store i64 %{{.*}}, ptr %{{.*}}, !enzyme_truetype ![[INT_STORE]]
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe fn copy_slice_ref(dst: *mut &'static [f64], src: *const &'static [f64]) {
    unsafe { *dst = *src }
}

// CHECK-LABEL: @copy_u8
// CHECK-NOT: enzyme_type
// CHECK-NOT: enzyme_truetype
// CHECK: ret void
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe fn copy_u8(dst: *mut u8, src: *const u8) {
    unsafe { *dst = *src }
}

// CHECK-LABEL: @copy_maybe_uninit
// CHECK-NOT: enzyme_type
// CHECK-NOT: enzyme_truetype
// CHECK: ret void
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe fn copy_maybe_uninit(dst: *mut MaybeUninit<u64>, src: *const MaybeUninit<u64>) {
    unsafe { *dst = *src }
}

// CHECK-DAG: ![[F64_LOAD]] = !{!"Unknown", i32 -1, ![[F64:[0-9]+]]}
// CHECK-DAG: ![[F64]] = !{!"Float@double"}
// CHECK-DAG: ![[F64_STORE]] = !{!"Float@double", i64 0}
// CHECK-DAG: ![[F32_LOAD]] = !{!"Unknown", i32 -1, ![[F32:[0-9]+]]}
// CHECK-DAG: ![[F32]] = !{!"Float@float"}
// CHECK-DAG: ![[F32_STORE]] = !{!"Float@float", i64 0}
// CHECK-DAG: ![[INT_LOAD]] = !{!"Unknown", i32 -1, ![[INT:[0-9]+]]}
// CHECK-DAG: ![[INT]] = !{!"Integer"}
// CHECK-DAG: ![[INT_STORE]] = !{!"Integer", i64 0}
// CHECK-DAG: ![[PTR_LOAD]] = !{!"Unknown", i32 -1, ![[PTR:[0-9]+]]}
// CHECK-DAG: ![[PTR]] = !{!"Pointer"}
// CHECK-DAG: ![[PTR_STORE]] = !{!"Pointer", i64 0}

fn main() {
    let (a, mut b) = (1.0f64, 0.0f64);
    let (c, mut d) = (1.0f32, 0.0f32);
    let (e, mut f) = (1usize, 0usize);
    let (g, mut h) = (&raw const a, std::ptr::null());
    let s: &'static [f64] = &[1.0];
    let mut t: &'static [f64] = &[];
    let (i, mut j) = (1u8, 0u8);
    let (k, mut l) = (MaybeUninit::new(1u64), MaybeUninit::uninit());
    unsafe {
        copy_f64(&mut b, &a);
        copy_f32(&mut d, &c);
        copy_usize(&mut f, &e);
        copy_ptr(&mut h, &g);
        copy_slice_ref(&mut t, &s);
        copy_u8(&mut j, &i);
        copy_maybe_uninit(&mut l, &k);
    }
}
