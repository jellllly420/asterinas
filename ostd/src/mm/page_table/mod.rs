// SPDX-License-Identifier: MPL-2.0

use core::{
    fmt::Debug,
    intrinsics::transmute_unchecked,
    marker::PhantomData,
    ops::Range,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::{
    nr_subpage_per_huge, page_prop::PageProperty, page_size, Paddr, PagingConstsTrait, PagingLevel,
    PodOnce, Vaddr,
};
use crate::{
    arch::mm::{PageTableEntry, PagingConsts},
    task::{atomic_mode::AsAtomicModeGuard, disable_preempt},
    util::marker::SameSizeAs,
    Pod,
};

mod node;
use node::*;
pub mod cursor;
pub(crate) use cursor::PageTableItem;
pub use cursor::{Cursor, CursorMut};
#[cfg(ktest)]
mod test;

pub(crate) mod boot_pt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageTableError {
    /// The provided virtual address range is invalid.
    InvalidVaddrRange(Vaddr, Vaddr),
    /// The provided virtual address is invalid.
    InvalidVaddr(Vaddr),
    /// Using virtual address not aligned.
    UnalignedVaddr,
}

/// This is a compile-time technique to force the frame developers to distinguish
/// between the kernel global page table instance, process specific user page table
/// instance, and device page table instances.
pub trait PageTableMode: Clone + Debug + 'static {
    /// The range of virtual addresses that the page table can manage.
    const VADDR_RANGE: Range<Vaddr>;

    /// Check if the given range is covered by the valid virtual address range.
    fn covers(r: &Range<Vaddr>) -> bool {
        Self::VADDR_RANGE.start <= r.start && r.end <= Self::VADDR_RANGE.end
    }
}

#[derive(Clone, Debug)]
pub struct UserMode {}

impl PageTableMode for UserMode {
    const VADDR_RANGE: Range<Vaddr> = 0..super::MAX_USERSPACE_VADDR;
}

#[derive(Clone, Debug)]
pub struct KernelMode {}

impl PageTableMode for KernelMode {
    const VADDR_RANGE: Range<Vaddr> = super::KERNEL_VADDR_RANGE;
}

// Here are some const values that are determined by the paging constants.

/// The number of virtual address bits used to index a PTE in a page.
const fn nr_pte_index_bits<C: PagingConstsTrait>() -> usize {
    nr_subpage_per_huge::<C>().ilog2() as usize
}

/// The index of a VA's PTE in a page table node at the given level.
const fn pte_index<C: PagingConstsTrait>(va: Vaddr, level: PagingLevel) -> usize {
    (va >> (C::BASE_PAGE_SIZE.ilog2() as usize + nr_pte_index_bits::<C>() * (level as usize - 1)))
        & (nr_subpage_per_huge::<C>() - 1)
}

/// A handle to a page table.
/// A page table can track the lifetime of the mapped physical pages.
#[derive(Debug)]
pub struct PageTable<
    M: PageTableMode,
    E: PageTableEntryTrait = PageTableEntry,
    C: PagingConstsTrait = PagingConsts,
> {
    root: PageTableNode<E, C>,
    _phantom: PhantomData<M>,
}

impl PageTable<UserMode> {
    pub fn activate(&self) {
        // SAFETY: The usermode page table is safe to activate since the kernel
        // mappings are shared.
        unsafe {
            self.root.activate();
        }
    }
}

impl PageTable<KernelMode> {
    /// Create a new kernel page table.
    pub(crate) fn new_kernel_page_table() -> Self {
        let kpt = Self::empty();

        // Make shared the page tables mapped by the root table in the kernel space.
        {
            let preempt_guard = disable_preempt();
            let mut root_node = kpt.root.borrow().lock(&preempt_guard);

            const NR_PTES_PER_NODE: usize = nr_subpage_per_huge::<PagingConsts>();
            let kernel_space_range = NR_PTES_PER_NODE / 2..NR_PTES_PER_NODE;

            for i in kernel_space_range {
                let mut root_entry = root_node.entry(i);
                let is_tracked = if super::kspace::should_map_as_tracked(
                    i * page_size::<PagingConsts>(PagingConsts::NR_LEVELS - 1),
                ) {
                    MapTrackingStatus::Tracked
                } else {
                    MapTrackingStatus::Untracked
                };
                let _ = root_entry
                    .alloc_if_none(&preempt_guard, is_tracked)
                    .unwrap();
            }
        }

        kpt
    }

    /// Create a new user page table.
    ///
    /// This should be the only way to create the user page table, that is to
    /// duplicate the kernel page table with all the kernel mappings shared.
    pub fn create_user_page_table(&self) -> PageTable<UserMode> {
        let new_root =
            PageTableNode::alloc(PagingConsts::NR_LEVELS, MapTrackingStatus::NotApplicable);

        let preempt_guard = disable_preempt();
        let mut root_node = self.root.borrow().lock(&preempt_guard);
        let mut new_node = new_root.borrow().lock(&preempt_guard);

        // Make a shallow copy of the root node in the kernel space range.
        // The user space range is not copied.
        const NR_PTES_PER_NODE: usize = nr_subpage_per_huge::<PagingConsts>();

        for i in NR_PTES_PER_NODE / 2..NR_PTES_PER_NODE {
            let root_entry = root_node.entry(i);
            let child = root_entry.to_ref();
            let Child::PageTableRef(pt) = child else {
                panic!("The kernel page table doesn't contain shared nodes");
            };
            let pt_cloned = pt.clone();

            let _ = new_node
                .entry(i)
                .replace(Child::PageTable(crate::sync::RcuDrop::new(pt_cloned)));
        }
        drop(new_node);

        PageTable::<UserMode> {
            root: new_root,
            _phantom: PhantomData,
        }
    }

    /// Protect the given virtual address range in the kernel page table.
    ///
    /// This method flushes the TLB entries when doing protection.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the protection operation does not affect
    /// the memory safety of the kernel.
    pub unsafe fn protect_flush_tlb(
        &self,
        vaddr: &Range<Vaddr>,
        mut op: impl FnMut(&mut PageProperty),
    ) -> Result<(), PageTableError> {
        let preempt_guard = disable_preempt();
        let mut cursor = CursorMut::new(self, &preempt_guard, vaddr)?;
        // SAFETY: The safety is upheld by the caller.
        while let Some(range) =
            unsafe { cursor.protect_next(vaddr.end - cursor.virt_addr(), &mut op) }
        {
            crate::arch::mm::tlb_flush_addr(range.start);
        }
        Ok(())
    }
}

impl<M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait> PageTable<M, E, C> {
    /// Create a new empty page table.
    ///
    /// Useful for the IOMMU page tables only.
    pub fn empty() -> Self {
        PageTable {
            root: PageTableNode::<E, C>::alloc(C::NR_LEVELS, MapTrackingStatus::NotApplicable),
            _phantom: PhantomData,
        }
    }

    pub(in crate::mm) unsafe fn first_activate_unchecked(&self) {
        // SAFETY: The safety is upheld by the caller.
        unsafe { self.root.first_activate() };
    }

    /// The physical address of the root page table.
    ///
    /// Obtaining the physical address of the root page table is safe, however, using it or
    /// providing it to the hardware will be unsafe since the page table node may be dropped,
    /// resulting in UAF.
    pub fn root_paddr(&self) -> Paddr {
        self.root.start_paddr()
    }

    pub unsafe fn map(
        &self,
        vaddr: &Range<Vaddr>,
        paddr: &Range<Paddr>,
        prop: PageProperty,
    ) -> Result<(), PageTableError> {
        let preempt_guard = disable_preempt();
        let mut cursor = self.cursor_mut(&preempt_guard, vaddr)?;
        // SAFETY: The safety is upheld by the caller.
        unsafe { cursor.map_pa(paddr, prop) };
        Ok(())
    }

    /// Query about the mapping of a single byte at the given virtual address.
    ///
    /// Note that this function may fail reflect an accurate result if there are
    /// cursors concurrently accessing the same virtual address range, just like what
    /// happens for the hardware MMU walk.
    #[cfg(ktest)]
    pub fn query(&self, vaddr: Vaddr) -> Option<(Paddr, PageProperty)> {
        // SAFETY: The root node is a valid page table node so the address is valid.
        unsafe { page_walk::<E, C>(self.root_paddr(), vaddr) }
    }

    /// Create a new cursor exclusively accessing the virtual address range for mapping.
    ///
    /// If another cursor is already accessing the range, the new cursor may wait until the
    /// previous cursor is dropped.
    pub fn cursor_mut<'rcu, G: AsAtomicModeGuard>(
        &'rcu self,
        guard: &'rcu G,
        va: &Range<Vaddr>,
    ) -> Result<CursorMut<'rcu, M, E, C>, PageTableError> {
        CursorMut::new(self, guard.as_atomic_mode_guard(), va)
    }

    /// Create a new cursor exclusively accessing the virtual address range for querying.
    ///
    /// If another cursor is already accessing the range, the new cursor may wait until the
    /// previous cursor is dropped. The modification to the mapping by the cursor may also
    /// block or be overridden by the mapping of another cursor.
    pub fn cursor<'rcu, G: AsAtomicModeGuard>(
        &'rcu self,
        guard: &'rcu G,
        va: &Range<Vaddr>,
    ) -> Result<Cursor<'rcu, M, E, C>, PageTableError> {
        Cursor::new(self, guard.as_atomic_mode_guard(), va)
    }

    /// Create a new reference to the same page table.
    /// The caller must ensure that the kernel page table is not copied.
    /// This is only useful for IOMMU page tables. Think twice before using it in other cases.
    pub unsafe fn shallow_copy(&self) -> Self {
        PageTable {
            root: self.root.clone(),
            _phantom: PhantomData,
        }
    }
}

/// A software emulation of the MMU address translation process.
/// It returns the physical address of the given virtual address and the mapping info
/// if a valid mapping exists for the given virtual address.
///
/// # Safety
///
/// The caller must ensure that the root_paddr is a valid pointer to the root
/// page table node.
///
/// # Notes on the page table free-reuse-then-read problem
///
/// Because neither the hardware MMU nor the software page walk method
/// would get the locks of the page table while reading, they can enter
/// a to-be-recycled page table node and read the page table entries
/// after the node is recycled and reused.
///
/// To mitigate this problem, the page table nodes are by default not
/// actively recycled, until we find an appropriate solution.
#[cfg(ktest)]
pub(super) unsafe fn page_walk<E: PageTableEntryTrait, C: PagingConstsTrait>(
    root_paddr: Paddr,
    vaddr: Vaddr,
) -> Option<(Paddr, PageProperty)> {
    use super::paddr_to_vaddr;

    let _guard = crate::trap::disable_local();

    let mut cur_level = C::NR_LEVELS;
    let mut cur_pte = {
        let node_addr = paddr_to_vaddr(root_paddr);
        let offset = pte_index::<C>(vaddr, cur_level);
        // SAFETY: The offset does not exceed the value of PAGE_SIZE.
        unsafe { (node_addr as *const E).add(offset).read() }
    };

    while cur_level > 1 {
        if !cur_pte.is_present() {
            return None;
        }

        if cur_pte.is_last(cur_level) {
            debug_assert!(cur_level <= C::HIGHEST_TRANSLATION_LEVEL);
            break;
        }

        cur_level -= 1;
        cur_pte = {
            let node_addr = paddr_to_vaddr(cur_pte.paddr());
            let offset = pte_index::<C>(vaddr, cur_level);
            // SAFETY: The offset does not exceed the value of PAGE_SIZE.
            unsafe { (node_addr as *const E).add(offset).read() }
        };
    }

    if cur_pte.is_present() {
        Some((
            cur_pte.paddr() + (vaddr & (page_size::<C>(cur_level) - 1)),
            cur_pte.prop(),
        ))
    } else {
        None
    }
}

/// The interface for defining architecture-specific page table entries.
///
/// Note that a default PTE should be a PTE that points to nothing.
pub trait PageTableEntryTrait:
    Clone + Copy + Debug + Default + Pod + PodOnce + SameSizeAs<usize> + Sized + Send + Sync + 'static
{
    /// Create a set of new invalid page table flags that indicates an absent page.
    ///
    /// Note that currently the implementation requires an all zero PTE to be an absent PTE.
    fn new_absent() -> Self {
        Self::default()
    }

    /// If the flags are present with valid mappings.
    ///
    /// For PTEs created by [`Self::new_absent`], this method should return
    /// false. And for PTEs created by [`Self::new_page`] or [`Self::new_pt`]
    /// and modified with [`Self::set_prop`] this method should return true.
    fn is_present(&self) -> bool;

    /// Create a new PTE with the given physical address and flags that map to a page.
    fn new_page(paddr: Paddr, level: PagingLevel, prop: PageProperty) -> Self;

    /// Create a new PTE that map to a child page table.
    fn new_pt(paddr: Paddr) -> Self;

    /// Get the physical address from the PTE.
    /// The physical address recorded in the PTE is either:
    /// - the physical address of the next level page table;
    /// - or the physical address of the page it maps to.
    fn paddr(&self) -> Paddr;

    fn prop(&self) -> PageProperty;

    /// Set the page property of the PTE.
    ///
    /// This will be only done if the PTE is present. If not, this method will
    /// do nothing.
    fn set_prop(&mut self, prop: PageProperty);

    /// If the PTE maps a page rather than a child page table.
    ///
    /// The level of the page table the entry resides is given since architectures
    /// like amd64 only uses a huge bit in intermediate levels.
    fn is_last(&self, level: PagingLevel) -> bool;

    /// Converts the PTE into its corresponding `usize` value.
    fn as_usize(self) -> usize {
        // SAFETY: `Self` is `Pod` and has the same memory representation as `usize`.
        unsafe { transmute_unchecked(self) }
    }

    /// Converts a usize `pte_raw` into a PTE.
    fn from_usize(pte_raw: usize) -> Self {
        // SAFETY: `Self` is `Pod` and has the same memory representation as `usize`.
        unsafe { transmute_unchecked(pte_raw) }
    }
}

/// Loads a page table entry with an atomic instruction.
///
/// # Safety
///
/// The safety preconditions are same as those of [`AtomicUsize::from_ptr`].
pub unsafe fn load_pte<E: PageTableEntryTrait>(ptr: *mut E, ordering: Ordering) -> E {
    // SAFETY: The safety is upheld by the caller.
    let atomic = unsafe { AtomicUsize::from_ptr(ptr.cast()) };
    let pte_raw = atomic.load(ordering);
    E::from_usize(pte_raw)
}

/// Stores a page table entry with an atomic instruction.
///
/// # Safety
///
/// The safety preconditions are same as those of [`AtomicUsize::from_ptr`].
pub unsafe fn store_pte<E: PageTableEntryTrait>(ptr: *mut E, new_val: E, ordering: Ordering) {
    let new_raw = new_val.as_usize();
    // SAFETY: The safety is upheld by the caller.
    let atomic = unsafe { AtomicUsize::from_ptr(ptr.cast()) };
    atomic.store(new_raw, ordering)
}
