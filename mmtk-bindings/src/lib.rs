use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use lazy_static::lazy_static;
use mmtk::vm::VMBinding;
use mmtk::{Mutator, MMTK};

pub mod active_plan;
pub mod api;
pub mod collection;
pub mod object_model;
pub mod reference_glue;
pub mod scanning;
pub mod slot;

pub type OCamlSlot = crate::slot::FieldSlot;
pub type OCamlSlice = crate::slot::UnimplementedMemorySlice;

#[derive(Default)]
pub struct OCamlVM;

// Documentation: https://docs.mmtk.io/api/mmtk/vm/trait.VMBinding.html
impl VMBinding for OCamlVM {
    type VMObjectModel = object_model::VMObjectModel;
    type VMScanning = scanning::VMScanning;
    type VMCollection = collection::VMCollection;
    type VMActivePlan = active_plan::VMActivePlan;
    type VMReferenceGlue = reference_glue::VMReferenceGlue;
    type VMSlot = OCamlSlot;
    type VMMemorySlice = OCamlSlice;

    /// Allowed maximum alignment in bytes.
    const MAX_ALIGNMENT: usize = 1 << 6;
}

use mmtk::util::{Address, ObjectReference};

// TODO(MMTk): Is this needed?
impl OCamlVM {
    pub fn object_start_to_ref(start: Address) -> ObjectReference {
        // Safety: start is the allocation result, and it should not be zero with an offset.
        unsafe {
            ObjectReference::from_raw_address_unchecked(
                start + crate::object_model::OBJECT_REF_OFFSET,
            )
        }
    }
}

pub static SINGLETON: OnceLock<Box<MMTK<OCamlVM>>> = OnceLock::new();

fn mmtk() -> &'static MMTK<OCamlVM> {
    SINGLETON.get().unwrap()
}

struct Roots(ObjectReference);

// TODO(MMTk): what else to use here?
// TODO(MMTk): Add an enum tracking thread status = runtime sleep, runtime active, mmtk sleep,
// mmtk active
#[derive(Debug)]
struct MutatorState {
    mutator: *mut Mutator<OCamlVM>,
}

// TODO(MMTk): Safety
unsafe impl Sync for MutatorState {}
unsafe impl Send for MutatorState {}

lazy_static! {
    // TODO(MMTk): Register global roots
    static ref GLOBAL_ROOTS: RwLock<Vec<Roots>> = RwLock::new(Vec::new());
    static ref MUTATORS: RwLock<HashMap<Address, MutatorState>> = RwLock::new(HashMap::new());
}
