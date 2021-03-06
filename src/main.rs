mod graphics;
mod repaint_signaler;
mod marching_cubes;
mod ui;

use std::sync::{Arc, Mutex};
use std::f32::consts;

use threemf::write::write;
use winit::{
    dpi::{PhysicalSize, PhysicalPosition},
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};
use futures::executor::block_on;
use glam::{vec3, Vec3, Quat};

use shader::ShaderConstants;
use shader::model::{model, ModelConstants};
use graphics::GraphicsState;
pub use repaint_signaler::RepaintSignaler;
use marching_cubes::marching_cubes;
pub use ui::UI;

const SPEED: f32 = 0.01;
const SENSITIVITY: f32 = 0.001;

struct State {
    size: winit::dpi::PhysicalSize<u32>,

    horizontal_rotation: f32,
    vertical_rotation: f32,
    focus: Vec3,
    offset: f32,

    input_dir: Vec3,

    dragging: bool,

    left: bool, right: bool,
    forward: bool, back: bool,
    up: bool, down: bool,
}

impl State {
    fn new(window: &Window) -> Self {
        let size = window.inner_size();

        Self {
            size,
            horizontal_rotation: 0.0,
            vertical_rotation: 0.0,
            focus: Vec3::ZERO,
            offset: 3.0,

            input_dir: Vec3::ZERO,

            dragging: false,

            left: false, right: false,
            forward: false, back: false,
            up: false, down: false,
        }
    }

    fn resize(&mut self, new_size: PhysicalSize<u32>) {
        self.size = new_size;
    }

    fn focus_changed(&mut self, focused: bool) {
        if !focused {
            self.dragging = false;
        }
    }

    fn mouse_input(&mut self, state: &ElementState) {
        if *state == ElementState::Pressed {
            self.dragging = true;
        } else {
            self.dragging = false;
        }
    }

    fn mouse_wheel(&mut self, delta: &MouseScrollDelta) {
        if let MouseScrollDelta::LineDelta(_x, y) = delta {
            self.offset -= y * 0.1;
        }
    }

    fn mouse_moved(&mut self, (delta_x, delta_y): (f64, f64)) {
        if self.dragging {
            self.horizontal_rotation -= delta_x as f32 * SENSITIVITY;

            let vertical_range = consts::FRAC_PI_2 - consts::FRAC_PI_8 / 4.0;
            self.vertical_rotation += delta_y as f32 * SENSITIVITY;
            self.vertical_rotation = self.vertical_rotation.min(vertical_range).max(-vertical_range);
        }
    }

    fn keyboard_input(&mut self, event: &KeyboardInput) {
        if event.state == ElementState::Pressed {
            match event.virtual_keycode {
                Some(VirtualKeyCode::D) => self.right = true,
                Some(VirtualKeyCode::A) => self.left = true,
                Some(VirtualKeyCode::W) => self.forward = true,
                Some(VirtualKeyCode::S) => self.back = true,
                Some(VirtualKeyCode::Space) | Some(VirtualKeyCode::Back) => self.up = true,
                Some(VirtualKeyCode::LShift) | Some(VirtualKeyCode::Grave) => self.down = true,
                _ => {}
            }
        } else if event.state == ElementState::Released {
            match event.virtual_keycode {
                Some(VirtualKeyCode::D) => self.right = false,
                Some(VirtualKeyCode::A) => self.left = false,
                Some(VirtualKeyCode::W) => self.forward = false,
                Some(VirtualKeyCode::S) => self.back = false,
                Some(VirtualKeyCode::Space) | Some(VirtualKeyCode::Back) => self.up = false,
                Some(VirtualKeyCode::LShift) | Some(VirtualKeyCode::Grave) => self.down = false,
                _ => {}
            }
        }
    }

    fn update(&mut self) {
        let camera_forward_vec = 
            Quat::from_rotation_y(self.horizontal_rotation) * 
            Quat::from_rotation_x(self.vertical_rotation) * 
            Vec3::Z;

        let up_vec = Vec3::Y;
        let right_vec = camera_forward_vec.cross(Vec3::Y);
        let forward_vec = up_vec.cross(right_vec);

        let horizontal = 
            (if self.left { -1.0 } else { 0.0 } +
            if self.right { 1.0 } else { 0.0 }) * right_vec;

        let aligned =
            (if self.back { -1.0 } else { 0.0 } +
            if self.forward { 1.0 } else { 0.0 }) * forward_vec;

        let vertical =
            (if self.down { -1.0 } else { 0.0 } +
            if self.up { 1.0 } else { 0.0 }) * up_vec;

        self.input_dir = horizontal + aligned + vertical;
        if self.input_dir.length() > 0.0 {
            self.input_dir = self.input_dir.normalize();
        }

        self.focus += self.input_dir * SPEED;
    }

    fn construct_constants(&mut self, model_constants: ModelConstants) -> ShaderConstants {
        let forward_vec = 
            Quat::from_rotation_y(self.horizontal_rotation) * 
            Quat::from_rotation_x(self.vertical_rotation) * 
            Vec3::Z;

        let position = self.focus - forward_vec * self.offset;
        let sun =
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2 / 10.0) *
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 / 10.0) *
            -forward_vec;

        ShaderConstants {
            pixel_width: self.size.width,
            pixel_height: self.size.height,
            screen_width: 0.1,
            screen_height: 0.1,
            screen_depth: 0.1,
            position: position.into(),
            forward: forward_vec.into(),
            sun: sun.into(),
            model_constants
        }
    }
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    let repaint_signaler = Arc::new(RepaintSignaler(Mutex::new(event_loop.create_proxy())));

    let mut graphics_state = block_on(GraphicsState::new(&window, repaint_signaler));
    let mut game_state = State::new(&window);
    let mut mouse_position = PhysicalPosition::new(0.0, 0.0);
    let mut drag_start = None;

    event_loop.run(move |event, _, control_flow| {
        if graphics_state.handle_event(&window, &event, control_flow, |model_constants| game_state.construct_constants(model_constants)) {
            return;
        }

        match event {
            Event::WindowEvent {
                ref event,
                window_id,
            } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::Focused(focused) => {
                    game_state.focus_changed(*focused);
                },
                WindowEvent::CursorMoved { position, .. } => {
                    mouse_position = *position;
                    if let Some(drag_start) = drag_start {
                        window.set_cursor_position(drag_start).expect("Could not set cursor position");
                    }
                },
                WindowEvent::MouseInput { state, .. } => {
                    if *state == ElementState::Pressed {
                        window.set_cursor_grab(true).expect("Could not grab cursor");
                        drag_start = Some(mouse_position.clone());
                    } else {
                        window.set_cursor_grab(false).expect("Could not release cursor");
                        window.set_cursor_position(drag_start.unwrap()).expect("Could not set cursor position");
                        drag_start = None;
                    }
                    game_state.mouse_input(state)
                },
                WindowEvent::MouseWheel { delta, .. } => {
                    game_state.mouse_wheel(delta);
                },
                WindowEvent::KeyboardInput { input, .. } => match input {
                    KeyboardInput {
                        state: ElementState::Pressed,
                        virtual_keycode: Some(VirtualKeyCode::Escape),
                        ..
                    } => {
                        game_state.focus_changed(false);
                    },
                    KeyboardInput {
                        state: ElementState::Pressed,
                        virtual_keycode: Some(VirtualKeyCode::M),
                        ..
                    } => {
                        println!("Exporting mesh");
                        let model_shape = model(graphics_state.model_constants());
                        let mesh = marching_cubes(model_shape, 5.0, 0.01);
                        write([".", "test.3mf"].iter().collect(), &mesh).expect("Could not write model to file");
                        println!("Mesh exported");
                    },
                    keyboard_input => game_state.keyboard_input(keyboard_input),
                },
                WindowEvent::Resized(physical_size) => {
                    game_state.resize(*physical_size);
                },
                _ => {}
            },
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion {
                    delta
                },
                ..
            } => {
                game_state.mouse_moved(delta);
            },
            Event::RedrawRequested(_) => {
                game_state.update();
            },
            _ => {}
        };
    });
}
