use std::collections::HashSet;
use std::hint;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::thread::{self, ThreadId};

use crate::active_plan::VMActivePlan;
use crate::OCamlVM;
use crate::{mmtk, MUTATORS};

use lazy_static::lazy_static;
use mmtk::memory_manager::start_worker;
use mmtk::util::opaque_pointer::*;
use mmtk::util::Address;
use mmtk::vm::GCThreadContext;
use mmtk::vm::{ActivePlan, Collection};
use mmtk::Mutator;

pub struct VMCollection {}

lazy_static! {
    static ref GC_THREADS: RwLock<HashSet<ThreadId>> = RwLock::new(HashSet::new());
}

static WANTS_TO_STOP: AtomicBool = AtomicBool::new(false);
static WORLD_HAS_STOPPED: AtomicBool = AtomicBool::new(false);
static GC_OVER: AtomicBool = AtomicBool::new(false);

// Documentation: https://docs.mmtk.io/api/mmtk/vm/collection/trait.Collection.html
impl Collection<OCamlVM> for VMCollection {
    fn stop_all_mutators<F>(tls: VMWorkerThread, mut mutator_visitor: F)
    where
        F: FnMut(&'static mut Mutator<OCamlVM>),
    {
        log::info!("Worker {} stopping all mutators", tls.0 .0.to_address());
        WANTS_TO_STOP.store(true, Ordering::SeqCst);

        // TODO(MMTk): Fix STW logic

        // TODO(MMTk): Add logging

        // Block for the world to stop all the mutators
        // TODO(MMTk): Figure out memory ordering
        while !WORLD_HAS_STOPPED.load(Ordering::SeqCst) {
            // TODO(MMTk): Add a condition variable to put this thread to sleep, to be woken up
            // by the runtime when all threads have slept themselves.
            hint::spin_loop();
        }

        log::info!("World has stopped!");
        for mutator in VMActivePlan::mutators() {
            log::trace!(
                "visiting mutator: {}",
                mutator.mutator_tls.0 .0.to_address()
            );
            mutator_visitor(mutator);
        }
    }

    // TODO(MMTk): Use tls for logging which GC thread requested it
    fn resume_mutators(tls: VMWorkerThread) {
        log::trace!("Resuming all mutators by worker {}", tls.0 .0.to_address());
        WANTS_TO_STOP.store(false, Ordering::SeqCst);
        GC_OVER.store(true, Ordering::SeqCst);
    }

    fn block_for_gc(tls: VMMutatorThread) {
        log::trace!("Mutator {} blocked for GC", tls.0 .0.to_address());

        // This is the only mutator, Stop the world
        // TODO(MMTk): Better mechanism to communicate this to the runtime
        if MUTATORS.read().unwrap().len() == 1 {
            WORLD_HAS_STOPPED.store(true, Ordering::SeqCst);
        }

        while !GC_OVER.load(Ordering::SeqCst) {
            // TODO(MMTk): Add a condition variable to put this thread to sleep, to be woken up
            // by the runtime when all threads have slept themselves.
            hint::spin_loop();
        }

        GC_OVER.store(false, Ordering::SeqCst);
    }

    // We use threads internal to MMTk and don't expose it to/expect it from the VM
    // Note that the thread may live up to the lifetime of the entire program, it will
    // be used internally by MMTk to service GC work packets
    fn spawn_gc_thread(_tls: VMThread, ctx: GCThreadContext<OCamlVM>) {
        let _ = thread::Builder::new()
            .name("MMTk Worker".to_string())
            .spawn(move || {
                register_current_thread();

                // Start the worker loop
                // We don't really need this to be a valid address, just something unique
                let worker_tls = VMWorkerThread(VMThread(OpaquePointer::from_address(unsafe {
                    Address::from_usize(thread_id::get())
                })));

                log::debug!("Worker spawned with tls {:#?}", worker_tls);
                match ctx {
                    GCThreadContext::Worker(w) => start_worker(mmtk(), worker_tls, w),
                }

                unregister_current_thread();
            });
    }
}

fn register_current_thread() {
    let id = std::thread::current().id();
    GC_THREADS.write().unwrap().insert(id);
}

fn unregister_current_thread() {
    let id = std::thread::current().id();
    GC_THREADS.write().unwrap().remove(&id);
}

#[no_mangle]
pub extern "C" fn world_has_stopped() {
    // TODO(MMTk): Figure out memory ordering, Julia uses SeqCst
    WORLD_HAS_STOPPED.store(true, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn wants_to_stop() -> bool {
    // TODO(MMTk): Expose the boolean to VM directly?
    WANTS_TO_STOP.load(Ordering::SeqCst)
}
