use super::*;
struct Memory(Vec<u8>);
impl Buffer for Memory {
    fn capacity(&self) -> usize {
        self.0.len()
    }
    fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<()> {
        let end = offset
            .checked_add(bytes.len())
            .ok_or_else(|| anyhow::anyhow!("overflow"))?;
        self.0
            .get_mut(offset..end)
            .ok_or_else(|| anyhow::anyhow!("bounds"))?
            .copy_from_slice(bytes);
        Ok(())
    }
    fn read(&self, offset: usize, length: usize) -> Option<&[u8]> {
        self.0.get(offset..offset.checked_add(length)?)
    }
}
#[test]
fn t697_defaults_auto_manual_and_configuration_limits() {
    let request = Request::default();
    assert_eq!(request.buffer_mib, 1);
    for cpus in 1..=256 {
        let plan = request.plan(cpus).unwrap();
        assert_eq!(plan.buffer_bytes(), MIB);
        assert_eq!(plan.worker_capacity(), cpus.saturating_sub(2).clamp(1, 128));
    }
    assert!(request.plan(0).is_err());
    assert_eq!(request.plan(u32::MAX).unwrap().worker_capacity(), 128);
    for workers in (0..=256).chain([u32::MAX]) {
        let result = Request { workers, ..request }.plan(4);
        assert_eq!(result.is_ok(), workers <= 128);
        if (1..=128).contains(&workers) {
            assert_eq!(result.unwrap().worker_capacity(), workers);
        }
    }
    for buffer_mib in (0..=256).chain([u32::MAX]) {
        let result = Request {
            buffer_mib,
            ..request
        }
        .plan(4);
        assert_eq!(result.is_ok(), PIPE_CAPACITIES_MIB.contains(&buffer_mib));
        if let Ok(plan) = result {
            assert_eq!(plan.buffer_bytes(), buffer_mib as usize * MIB);
        }
    }
}
#[test]
fn t697_effective_capacity_is_verified_separately_and_dirty_work_is_bounded() {
    let plan = Request {
        buffer_mib: 8,
        workers: 8,
    }
    .plan(2)
    .unwrap();
    for bytes in [0, 1, MIB, 8 * MIB, 8 * MIB + 1] {
        let buffer = Memory(vec![0; bytes]);
        for started in [0, 1, 4, 8, 9, u32::MAX] {
            let result = plan.record(&buffer, started);
            assert_eq!(
                result.is_ok(),
                bytes > 0 && bytes <= 8 * MIB && started > 0 && started <= 8
            );
        }
    }
    let buffer = Memory(vec![0; MIB]);
    let report = plan.record(&buffer, 3).unwrap();
    assert_eq!(
        report.requested,
        Request {
            buffer_mib: 8,
            workers: 8
        }
    );
    assert_eq!(report.buffer_bytes, MIB);
    assert_eq!(report.worker_capacity, 3);
    for units in (0..=256).chain([u32::MAX]) {
        assert_eq!(report.active_workers(units), units.min(3));
    }
}
