use std::sync::Arc;

use winit::{
    application::ApplicationHandler,
    event::{MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

mod input;
mod render;

use input::Input;
use render::Renderer;

const TITLE: &str = "voxel-game";

/// One slot per tracked input, held or not, so a slot's position never depends
/// on what else is down.
const KEY_SLOTS: [(KeyCode, char); 4] = [
    (KeyCode::KeyW, 'W'),
    (KeyCode::KeyA, 'A'),
    (KeyCode::KeyS, 'S'),
    (KeyCode::KeyD, 'D'),
];
const MOUSE_SLOTS: [(MouseButton, char); 2] = [(MouseButton::Left, 'L'), (MouseButton::Right, 'R')];

const EMPTY_SLOT: char = '_';

fn slot(held: bool, label: char) -> char {
    if held { label } else { EMPTY_SLOT }
}

fn hud_title(input: &Input) -> String {
    let mut slots = String::with_capacity(KEY_SLOTS.len() + MOUSE_SLOTS.len());
    for (code, label) in KEY_SLOTS {
        slots.push(slot(input.is_down(code), label));
    }
    for (button, label) in MOUSE_SLOTS {
        slots.push(slot(input.is_mouse_down(button), label));
    }
    format!("{TITLE}  {slots}")
}

#[derive(Default)]
struct App {
    renderer: Option<Renderer>,
    input: Input,
    title: String,
}

impl App {
    /// Titles are re-set from input events, not the redraw loop, and only when
    /// the string actually changed — key auto-repeat alone must not cost a
    /// window-system round trip.
    fn sync_title(&mut self) {
        let title = hud_title(&self.input);
        if title != self.title {
            self.title = title;
            if let Some(renderer) = &self.renderer {
                renderer.window().set_title(&self.title);
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window_attributes = Window::default_attributes().with_title(TITLE);

        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        self.renderer = Some(Renderer::new(window).expect("renderer initialization failed"));
        self.sync_title();
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                println!("Closing window {window_id:?}");
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                event,
                device_id: _,
                is_synthetic: _,
            } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    self.input.on_key(key, event.state, event.repeat);
                    self.sync_title();
                }
            }
            WindowEvent::MouseInput {
                state,
                button,
                device_id: _,
            } => {
                self.input.on_mouse(button, state);
                self.sync_title();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.render();
                }
                // Frame boundary
                self.input.end_frame();
            }
            _ => (),
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    event_loop.run_app(&mut app)?;

    Ok(())
}
