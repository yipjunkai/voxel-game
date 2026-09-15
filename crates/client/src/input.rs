use std::collections::HashSet;

use winit::event::{ElementState, MouseButton};
use winit::keyboard::KeyCode;

#[derive(Default, Debug)]
pub struct Input {
    held: HashSet<KeyCode>,
    pressed: HashSet<KeyCode>,
    released: HashSet<KeyCode>,
    mouse_held: HashSet<MouseButton>,
    mouse_pressed: HashSet<MouseButton>,
    mouse_released: HashSet<MouseButton>,
}

impl Input {
    pub fn on_key(&mut self, code: KeyCode, state: ElementState, repeat: bool) {
        match state {
            // Auto-repeat is not a new press — the key never came up.
            ElementState::Pressed if !repeat => {
                self.held.insert(code);
                self.pressed.insert(code);
            }
            ElementState::Released => {
                self.held.remove(&code);
                self.released.insert(code);
            }
            ElementState::Pressed => {}
        }
    }

    /// Mouse buttons have no auto-repeat, so every press is a real press.
    pub fn on_mouse(&mut self, button: MouseButton, state: ElementState) {
        match state {
            ElementState::Pressed => {
                self.mouse_held.insert(button);
                self.mouse_pressed.insert(button);
            }
            ElementState::Released => {
                self.mouse_held.remove(&button);
                self.mouse_released.insert(button);
            }
        }
    }

    /// Held down right now. For continuous actions: walking, mining.
    pub fn is_down(&self, code: KeyCode) -> bool {
        self.held.contains(&code)
    }

    /// Went down this frame. For one-shot actions: jump, place block.
    #[allow(dead_code)]
    pub fn just_pressed(&self, code: KeyCode) -> bool {
        self.pressed.contains(&code)
    }

    #[allow(dead_code)]
    pub fn is_mouse_down(&self, button: MouseButton) -> bool {
        self.mouse_held.contains(&button)
    }

    #[allow(dead_code)]
    pub fn mouse_just_pressed(&self, button: MouseButton) -> bool {
        self.mouse_pressed.contains(&button)
    }

    /// Call after update+render, every frame.
    pub fn end_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
        self.mouse_pressed.clear();
        self.mouse_released.clear();
    }

    /// Decision made: not cleared on alt-tab.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.held.clear();
        self.pressed.clear();
        self.released.clear();
        self.mouse_held.clear();
        self.mouse_pressed.clear();
        self.mouse_released.clear();
    }
}
