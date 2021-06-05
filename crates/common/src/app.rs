use bevy_ecs::{
    component::Component,
    prelude::*,
    schedule::{RunOnce, StageLabel, SystemDescriptor},
};

use crate::{events::Events, gameloop::Timer};

pub struct App {
    timer: Timer,
    pub world: World,
    schedule: Schedule,
    render_schedule: Schedule,
    runner: Box<dyn Fn(App) + Send + Sync + 'static>,
}

impl App {
    pub fn builder() -> AppBuilder {
        AppBuilder::default().insert_non_send(simple_async_local_executor::Executor::default())
    }

    pub fn update(&mut self) {
        self.timer.update();
        if self.timer.tick() {
            self.tick();
        }
    }

    fn tick(&mut self) {
        self.schedule.run_once(&mut self.world);
    }

    pub fn render(&mut self) {
        self.render_schedule.run_once(&mut self.world);
    }

    pub fn run(mut self) {
        let runner = std::mem::replace(&mut self.runner, Box::new(run_once));
        (runner)(self);
    }
}

pub struct AppBuilder {
    world: World,
    schedule: Schedule,
    render_schedule: Schedule,
    runner: Box<dyn Fn(App) + Send + Sync>,
}

fn run_once(mut app: App) {
    app.update();
}

impl AppBuilder {
    fn new() -> Self {
        Self {
            world: World::new(),
            schedule: Schedule::default(),
            render_schedule: Schedule::default(),
            runner: Box::new(run_once),
        }
    }

    fn add_default_stages(self) -> Self {
        self.add_stage(
            CoreStage::Startup,
            Schedule::default()
                .with_run_criteria(RunOnce::default())
                .with_stage(StartupStage::Startup, default_stage()),
        )
        .add_stage(CoreStage::First, default_stage())
        .add_stage(CoreStage::PreUpdate, default_stage())
        .add_stage(CoreStage::Update, default_stage())
        .add_stage(CoreStage::PostUpdate, default_stage())
        .add_stage(CoreStage::Last, default_stage())
    }

    fn add_stage(mut self, label: impl StageLabel, stage: impl Stage) -> Self {
        self.schedule.add_stage(label, stage);
        self
    }

    pub fn insert_resource<T>(mut self, resource: T) -> Self
    where
        T: Component,
    {
        self.world.insert_resource(resource);
        self
    }

    pub fn insert_non_send<T>(mut self, resource: T) -> Self
    where
        T: 'static,
    {
        self.world.insert_non_send(resource);
        self
    }

    pub fn add_event<T>(self) -> Self
    where
        T: Component,
    {
        self.insert_resource(Events::<T>::default())
            .add_system_to_stage(CoreStage::First, Events::<T>::update_system.system())
    }

    pub fn add_startup_system(self, system: impl Into<SystemDescriptor>) -> Self {
        self.add_startup_system_to_stage(StartupStage::Startup, system)
    }

    pub fn add_startup_system_to_stage(
        mut self,
        stage_label: impl StageLabel,
        system: impl Into<SystemDescriptor>,
    ) -> Self {
        self.schedule
            .stage(CoreStage::Startup, |schedule: &mut Schedule| {
                schedule.add_system_to_stage(stage_label, system)
            });
        self
    }

    pub fn add_system_to_stage(
        mut self,
        label: impl StageLabel,
        system: impl Into<SystemDescriptor>,
    ) -> Self {
        self.schedule.add_system_to_stage(label, system);
        self
    }

    pub fn add_system(self, system: impl Into<SystemDescriptor>) -> Self {
        self.add_system_to_stage(CoreStage::Update, system)
    }

    pub fn set_runner(self, runner: impl Fn(App) + Send + Sync + 'static) -> Self {
        Self {
            runner: Box::new(runner),
            ..self
        }
    }

    pub fn add_plugin(self, mut plugin: impl Plugin) -> Self {
        plugin.build(self)
    }

    pub fn build(self) -> App {
        App {
            timer: Timer::new(60),
            schedule: self.schedule,
            render_schedule: self.render_schedule,
            world: self.world,
            runner: self.runner,
        }
    }

    pub fn run(self) {
        self.build().run()
    }
}

#[cfg(target_arch = "wasm32")]
fn default_stage() -> impl Stage {
    SystemStage::single_threaded()
}

#[cfg(not(target_arch = "wasm32"))]
fn default_stage() -> impl Stage {
    SystemStage::single_threaded()
}

impl Default for AppBuilder {
    fn default() -> Self {
        AppBuilder::new().add_default_stages()
    }
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, StageLabel)]
pub enum CoreStage {
    Startup,
    First,
    PreUpdate,
    Update,
    PostUpdate,
    Last,
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, StageLabel)]
pub enum StartupStage {
    Startup,
}

pub trait Plugin {
    fn build(&mut self, app: AppBuilder) -> AppBuilder;
}
