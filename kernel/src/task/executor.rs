use crate::arch::Local;
use conquer_once::spin::OnceCell;
use core::{
    cell::Cell,
    future::Future,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use maitake::{
    scheduler::{Injector, StaticScheduler, Stealer, TaskStub},
    task::JoinHandle,
};
use rand::{Rng, SeedableRng};

static SCHEDULER: Local<Cell<Option<&'static StaticScheduler>>> = Local::new(|| Cell::new(None));

static RUNTIME: Runtime = {
    const UNINIT_SCHEDULER: OnceCell<StaticScheduler> = OnceCell::uninit();

    Runtime {
        cores: [UNINIT_SCHEDULER; MAX_CORES],
        initialized: AtomicUsize::new(0),
        injector: {
            static STUB_TASK: TaskStub = TaskStub::new();
            unsafe { Injector::new_with_static_stub(&STUB_TASK) }
        },
    }
};

const MAX_CORES: usize = 32;

struct Runtime {
    cores: [OnceCell<StaticScheduler>; MAX_CORES],

    injector: Injector<&'static StaticScheduler>,
    initialized: AtomicUsize,
}

impl Runtime {
    fn active_cores(&self) -> usize {
        self.initialized.load(Ordering::Acquire)
    }

    fn new_scheduler(&self) -> (usize, &StaticScheduler) {
        let next = self.initialized.fetch_add(1, Ordering::AcqRel);
        assert!(next < MAX_CORES);
        let scheduler = self.cores[next]
            .try_get_or_init(StaticScheduler::new)
            .unwrap();
        (next, scheduler)
    }

    fn try_steal_from(
        &'static self,
        idx: usize,
    ) -> Option<Stealer<'static, &'static StaticScheduler>> {
        self.cores[idx].try_get().unwrap().try_steal().ok()
    }
}

pub struct Executor {
    id: usize,
    scheduler: &'static StaticScheduler,
    running: AtomicBool,
    rng: rand_xoshiro::Xoroshiro128PlusPlus,
}

impl Executor {
    pub fn new() -> Executor {
        let (id, scheduler) = RUNTIME.new_scheduler();
        Executor {
            id,
            scheduler,
            running: AtomicBool::new(false),
            rng: rand_xoshiro::Xoroshiro128PlusPlus::from_seed(
                <rand_xoshiro::Xoroshiro128PlusPlus as SeedableRng>::Seed::default(),
            ),
        }
    }

    #[inline]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    fn tick(&mut self) -> bool {
        let tick = self.scheduler.tick();

        super::timer::TIMER.advance_ticks(0);

        if tick.has_remaining {
            return true;
        }

        let stolen = self.try_steal();
        if stolen > 0 {
            return true;
        }

        false
    }

    pub fn run(&mut self) {
        struct CoreGuard;
        impl Drop for CoreGuard {
            fn drop(&mut self) {
                SCHEDULER.with(|scheduler| scheduler.set(None));
            }
        }

        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        SCHEDULER.with(|scheduler| scheduler.set(Some(self.scheduler)));
        let _unset = CoreGuard;

        loop {
            if self.tick() {
                continue;
            }

            if !self.is_running() {
                return;
            }

            crate::arch::enable_interrupts_and_halt();
        }
    }

    fn try_steal(&mut self) -> usize {
        const MAX_STEAL_ATTEMPTS: usize = 16;
        const MAX_STOLEN_PER_TICK: usize = 256;

        if let Ok(injector) = RUNTIME.injector.try_steal() {
            return injector.spawn_n(&self.scheduler, MAX_STOLEN_PER_TICK);
        }

        let mut attempts = 0;
        while attempts < MAX_STEAL_ATTEMPTS {
            let active_cores = RUNTIME.active_cores();

            if active_cores <= 1 {
                break;
            }

            let victim_idx = self.rng.gen_range(0..active_cores);

            if victim_idx == self.id {
                continue;
            }

            if let Some(victim) = RUNTIME.try_steal_from(victim_idx) {
                let num_steal =
                    core::cmp::min(victim.initial_task_count() / 2, MAX_STOLEN_PER_TICK);
                return victim.spawn_n(&self.scheduler, num_steal);
            } else {
                attempts += 1;
            }
        }

        if let Ok(injector) = RUNTIME.injector.try_steal() {
            injector.spawn_n(&self.scheduler, MAX_STOLEN_PER_TICK)
        } else {
            0
        }
    }
}

pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    SCHEDULER.with(|scheduler| {
        if let Some(scheduler) = scheduler.get() {
            scheduler.spawn(future)
        } else {
            RUNTIME.injector.spawn(future)
        }
    })
}

pub fn block_on<F>(future: F) -> F::Output
where
    F: Future + Send,
    F::Output: Send,
{
    use alloc::sync::Arc;
    use futures::task::{waker, ArcWake, Context, Poll};

    futures::pin_mut!(future);

    struct Waker;
    impl ArcWake for Waker {
        fn wake_by_ref(_arc_self: &Arc<Self>) {}
    }

    let waker = waker(Arc::new(Waker));
    let mut context = Context::from_waker(&waker);

    loop {
        super::timer::TIMER.advance_ticks(0);

        match Future::poll(future.as_mut(), &mut context) {
            Poll::Ready(v) => {
                return v;
            }
            Poll::Pending => {}
        }

        crate::arch::enable_interrupts_and_halt();
    }
}
