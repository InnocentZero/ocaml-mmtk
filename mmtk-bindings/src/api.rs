// All functions here are extern function. There is no point for marking them as unsafe.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use crate::mmtk;
use crate::OCamlSlot;
use crate::OCamlVM;
use crate::Roots;
use crate::GLOBAL_ROOTS;
use crate::MUTATORS;
use crate::SINGLETON;
use mmtk::memory_manager;
use mmtk::scheduler::GCWorker;
use mmtk::util::opaque_pointer::*;
use mmtk::util::options::PlanSelector;
use mmtk::util::{Address, ObjectReference};
use mmtk::AllocationSemantics;
use mmtk::Mutator;
use std::ffi::c_char;
use std::ffi::CStr;
use std::mem;

// This file exposes MMTk Rust API to the native code. This is not an exhaustive list of all the APIs.
// Most commonly used APIs are listed in https://docs.mmtk.io/api/mmtk/memory_manager/index.html. The binding can expose them here.

#[no_mangle]
pub fn mmtk_init(heap_size: u32, plan: *const c_char) {
    let mut builder = Box::new(mmtk::MMTKBuilder::new());

    let plan: &CStr = unsafe { CStr::from_ptr(plan) };
    let plan: &str = plan
        .to_str()
        .expect("Improperly formatted string passed by runtime");
    let plan = match plan {
        "nogc" => PlanSelector::NoGC,
        "marksweep" => PlanSelector::MarkSweep,
        "immix" => PlanSelector::Immix,
        "stickyimmix" => PlanSelector::StickyImmix,
        _ => panic!("Unknonwn plan {plan} passed from runtime"),
    };
    builder.options.plan.set(plan);

    builder
        .options
        .gc_trigger
        .set(mmtk::util::options::GCTriggerSelector::FixedHeapSize(
            heap_size as usize,
        ));

    // Create MMTK instance.
    let mmtk = memory_manager::mmtk_init::<OCamlVM>(&builder);

    SINGLETON.set(mmtk).unwrap_or_else(|_| {
        panic!("Failed to set SINGLETON");
    });
}

#[no_mangle]
pub extern "C" fn mmtk_bind_mutator(tls: VMMutatorThread) -> *mut Mutator<OCamlVM> {
    let mut mutator = memory_manager::bind_mutator(mmtk(), tls);
    MUTATORS.write().unwrap().insert(
        tls.0 .0.to_address(),
        crate::MutatorState {
            mutator: mutator.as_mut() as *mut Mutator<OCamlVM>,
        },
    );
    memory_manager::initialize_collection(mmtk(), tls.0);
    Box::into_raw(mutator)
}

#[no_mangle]
pub extern "C" fn mmtk_destroy_mutator(mutator: *mut Mutator<OCamlVM>) {
    MUTATORS
        .write()
        .unwrap()
        .remove(unsafe { &mutator.as_mut().unwrap().mutator_tls.0 .0.to_address() });
    memory_manager::destroy_mutator(unsafe { &mut *mutator });
    let _ = unsafe { Box::from_raw(mutator) };
}

#[no_mangle]
pub extern "C" fn mmtk_alloc(
    mutator: *mut Mutator<OCamlVM>,
    size: usize,
    align: usize,
    offset: usize,
    mut semantics: AllocationSemantics,
) -> Address {
    // TODO(MMTk): Move this to VM side?
    if size
        >= mmtk()
            .get_plan()
            .constraints()
            .max_non_los_default_alloc_bytes
    {
        semantics = AllocationSemantics::Los;
    }
    // TODO(MMTk): Track if the control of `mutator` is with MMTk in MUTATORS
    let alloc_addr =
        memory_manager::alloc::<OCamlVM>(unsafe { &mut *mutator }, size, align, offset, semantics);
    alloc_addr.add(mem::size_of::<OCamlSlot>())
}

#[no_mangle]
pub extern "C" fn mmtk_post_alloc(
    mutator: *mut Mutator<OCamlVM>,
    refer: ObjectReference,
    bytes: usize,
    tag: usize,
    semantics: AllocationSemantics,
) {
    let header_addr = refer.to_raw_address().sub(mem::size_of::<OCamlSlot>());
    let words = bytes / mem::size_of::<OCamlSlot>();
    // TODO(MMTk): safety
    unsafe {
        header_addr.store((words << 10) | tag);
    }
    // TODO(MMTk): Track if the control is with MMTk in MUTATORS
    memory_manager::post_alloc::<OCamlVM>(unsafe { &mut *mutator }, refer, bytes, semantics)
}

#[no_mangle]
pub extern "C" fn mmtk_start_worker(tls: VMWorkerThread, worker: *mut GCWorker<OCamlVM>) {
    // TODO(MMTk): When this is called, can this deadlock for that particular thread?
    let worker = unsafe { Box::from_raw(worker) };
    memory_manager::start_worker::<OCamlVM>(mmtk(), tls, worker)
}

#[no_mangle]
pub extern "C" fn mmtk_initialize_collection(tls: VMThread) {
    memory_manager::initialize_collection(mmtk(), tls)
}

#[no_mangle]
pub extern "C" fn mmtk_used_bytes() -> usize {
    memory_manager::used_bytes(mmtk())
}

#[no_mangle]
pub extern "C" fn mmtk_free_bytes() -> usize {
    memory_manager::free_bytes(mmtk())
}

#[no_mangle]
pub extern "C" fn mmtk_total_bytes() -> usize {
    memory_manager::total_bytes(mmtk())
}

#[no_mangle]
pub extern "C" fn mmtk_is_live_object(object: ObjectReference) -> bool {
    memory_manager::is_live_object(object)
}

#[no_mangle]
pub extern "C" fn mmtk_will_never_move(object: ObjectReference) -> bool {
    !object.is_movable()
}

#[no_mangle]
pub extern "C" fn mmtk_is_in_mmtk_spaces(object: ObjectReference) -> bool {
    memory_manager::is_in_mmtk_spaces(object)
}

#[no_mangle]
pub extern "C" fn mmtk_is_mapped_address(address: Address) -> bool {
    memory_manager::is_mapped_address(address)
}

#[no_mangle]
pub extern "C" fn mmtk_handle_user_collection_request(tls: VMMutatorThread) {
    memory_manager::handle_user_collection_request::<OCamlVM>(mmtk(), tls);
}

#[no_mangle]
pub extern "C" fn mmtk_add_weak_candidate(reff: ObjectReference) {
    memory_manager::add_weak_candidate(mmtk(), reff)
}

#[no_mangle]
pub extern "C" fn mmtk_add_soft_candidate(reff: ObjectReference) {
    memory_manager::add_soft_candidate(mmtk(), reff)
}

#[no_mangle]
pub extern "C" fn mmtk_add_phantom_candidate(reff: ObjectReference) {
    memory_manager::add_phantom_candidate(mmtk(), reff)
}

#[no_mangle]
pub extern "C" fn mmtk_harness_begin(tls: VMMutatorThread) {
    memory_manager::harness_begin(mmtk(), tls)
}

#[no_mangle]
pub extern "C" fn mmtk_harness_end() {
    memory_manager::harness_end(mmtk())
}

#[no_mangle]
pub extern "C" fn mmtk_starting_heap_address() -> Address {
    memory_manager::starting_heap_address()
}

#[no_mangle]
pub extern "C" fn mmtk_last_heap_address() -> Address {
    memory_manager::last_heap_address()
}

#[no_mangle]
pub extern "C" fn mmtk_register_global_root(reff: ObjectReference) {
    GLOBAL_ROOTS.write().unwrap().push(Roots(reff));
}
