//! What the machine under the renderer actually is.
//!
//! The core sorts a light rung out of a device class and a memory size, and only
//! the renderer can say either. wgpu names the class but keeps the size to itself,
//! so the second half is asked of Vulkan directly: the same loader, the same
//! physical device, the heaps the driver says the card owns outright.
//!
//! Both halves are a reading, not a promise. Anything that cannot be found is
//! reported as nothing found, and the core answers that with its own fallback
//! rather than with a guess made here.

use ash::{Entry, vk};
use carbine_core::DeviceKind;

/// What wgpu calls the device, as the core sorts it.
fn kind_of(device_type: wgpu::DeviceType) -> DeviceKind {
    match device_type {
        wgpu::DeviceType::Cpu => DeviceKind::Cpu,
        wgpu::DeviceType::IntegratedGpu => DeviceKind::Integrated,
        wgpu::DeviceType::DiscreteGpu => DeviceKind::Discrete,
        wgpu::DeviceType::VirtualGpu => DeviceKind::Virtual,
        wgpu::DeviceType::Other => DeviceKind::Other,
    }
}

/// The Vulkan class of the same device, so a physical device can be told from
/// another one with the same numbers on it.
fn vk_class_of(device_type: wgpu::DeviceType) -> Option<vk::PhysicalDeviceType> {
    Some(match device_type {
        wgpu::DeviceType::Cpu => vk::PhysicalDeviceType::CPU,
        wgpu::DeviceType::IntegratedGpu => vk::PhysicalDeviceType::INTEGRATED_GPU,
        wgpu::DeviceType::DiscreteGpu => vk::PhysicalDeviceType::DISCRETE_GPU,
        wgpu::DeviceType::VirtualGpu => vk::PhysicalDeviceType::VIRTUAL_GPU,
        wgpu::DeviceType::Other => return None,
    })
}

/// Bytes this card owns outright, or nothing at all when it will not say.
///
/// Only the heaps the driver marks as living on the device count: an integrated
/// part's host memory is the machine's, not the card's, and the core reads an
/// integrated device as unified whatever comes back here.
fn device_local_bytes(properties: &vk::PhysicalDeviceMemoryProperties) -> Option<u64> {
    let count = properties.memory_heap_count as usize;
    let heaps = &properties.memory_heaps[..count.min(vk::MAX_MEMORY_HEAPS)];
    let owned: u64 = heaps
        .iter()
        .filter(|heap| heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
        .map(|heap| heap.size)
        .sum();
    (owned > 0).then_some(owned)
}

/// Asks Vulkan how much memory the card wgpu chose owns. Every step of this can
/// fail on a real machine - no loader, no Vulkan driver, a device that was
/// hot-plugged out from under the answer - and every failure means the same
/// thing: nothing was learned.
fn probe_vram(info: &wgpu::AdapterInfo) -> Option<u64> {
    if info.backend != wgpu::Backend::Vulkan {
        return None;
    }
    let class = vk_class_of(info.device_type)?;
    let entry = unsafe { Entry::load() }.ok()?;
    let instance =
        unsafe { entry.create_instance(&vk::InstanceCreateInfo::default(), None) }.ok()?;
    let found = unsafe { instance.enumerate_physical_devices() }
        .ok()?
        .into_iter()
        .find(|device| {
            let properties = unsafe { instance.get_physical_device_properties(*device) };
            properties.vendor_id == info.vendor
                && properties.device_id == info.device
                && properties.device_type == class
        })
        .map(|device| {
            let memory = unsafe { instance.get_physical_device_memory_properties(device) };
            device_local_bytes(&memory)
        });
    unsafe { instance.destroy_instance(None) };
    found.flatten()
}

/// The whole reading of the device wgpu handed the renderer.
pub fn describe(adapter: &wgpu::Adapter) -> carbine_core::Gpu {
    let info = adapter.get_info();
    carbine_core::Gpu {
        kind: kind_of(info.device_type),
        vram: probe_vram(&info),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_class_means_what_the_core_sorts_it_as() {
        for (device_type, kind) in [
            (wgpu::DeviceType::Cpu, DeviceKind::Cpu),
            (wgpu::DeviceType::IntegratedGpu, DeviceKind::Integrated),
            (wgpu::DeviceType::DiscreteGpu, DeviceKind::Discrete),
            (wgpu::DeviceType::VirtualGpu, DeviceKind::Virtual),
            (wgpu::DeviceType::Other, DeviceKind::Other),
        ] {
            assert_eq!(kind_of(device_type), kind);
        }
    }

    #[test]
    fn only_memory_on_the_device_is_counted_as_its_own() {
        let mut memory = vk::PhysicalDeviceMemoryProperties::default();
        memory.memory_heap_count = 2;
        memory.memory_heaps[0] = vk::MemoryHeap::default()
            .size(4096)
            .flags(vk::MemoryHeapFlags::DEVICE_LOCAL);
        memory.memory_heaps[1] = vk::MemoryHeap::default()
            .size(1_000_000)
            .flags(vk::MemoryHeapFlags::empty());
        assert_eq!(device_local_bytes(&memory), Some(4096));

        // A heap list with nothing on the device in it is a card that owns none.
        memory.memory_heaps[0] = vk::MemoryHeap::default()
            .size(4096)
            .flags(vk::MemoryHeapFlags::empty());
        assert_eq!(device_local_bytes(&memory), None);
    }

    /// The renderer on this machine has to describe itself, and the answer has to
    /// be one the core can put a rung on.
    #[test]
    fn the_device_the_renderer_drew_on_can_be_read() {
        let gpu = crate::Gpu::new(crate::create_instance(), None).unwrap();
        let hardware = gpu.hardware();

        assert_eq!(hardware.kind, kind_of(gpu.adapter.get_info().device_type));
        assert!(DeviceKind::from_name(hardware.kind.name()).is_some());
        // A reading of zero bytes would be a bug, not a small card.
        assert!(hardware.vram.is_none_or(|bytes| bytes > 0));
    }
}
