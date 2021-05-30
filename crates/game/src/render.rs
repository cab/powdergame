use std::rc::Rc;

use bytemuck::{cast, cast_ref};
use js_sys::{ArrayBuffer, Float32Array};
use tracing::debug;
use ultraviolet::{projection::lh_yup::orthographic_gl, Mat4, Vec3, Vec4};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    HtmlCanvasElement, WebGl2RenderingContext, WebGlBuffer, WebGlProgram, WebGlShader,
    WebGlUniformLocation,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not initialize: {0}")]
    Initialization(String),
    #[error("js api error")]
    Js(JsValue),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Renderer {
    context: Rc<WebGl2RenderingContext>,
    pixel_pass: PixelPass,
    sprite_pass: SpritePass,
}

impl Renderer {
    pub fn new(canvas: &mut HtmlCanvasElement) -> Result<Self> {
        let context = Rc::new(
            canvas
                .get_context("webgl2")
                .map_err(Error::Js)?
                .ok_or_else(|| Error::Initialization("webgl2 context not available".to_string()))?
                .unchecked_into(),
        );
        let pixel_pass = PixelPass::new(Rc::clone(&context));
        let sprite_pass = SpritePass::new(Rc::clone(&context));
        Ok(Self {
            context,
            sprite_pass,
            pixel_pass,
        })
    }

    pub fn render(&self) {
        self.context.clear_color(0.0, 0.0, 0.0, 1.0);
        self.context.clear(WebGl2RenderingContext::COLOR_BUFFER_BIT);
        self.pixel_pass.render();
        self.sprite_pass.render();
    }
}

struct PixelPass {
    context: Rc<WebGl2RenderingContext>,
    position_buffer: WebGlBuffer,
    program: WebGlProgram,
    // vertex_position attribute location
    a_vertex_position: i32,
    u_projection: WebGlUniformLocation,
}

impl PixelPass {
    fn create_position_buffer(context: &WebGl2RenderingContext) -> WebGlBuffer {
        let buffer = context.create_buffer().unwrap();
        context.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&buffer));
        let positions: &[f32] = &[1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0];
        context.buffer_data_with_array_buffer_view(
            WebGl2RenderingContext::ARRAY_BUFFER,
            &Float32Array::from(positions),
            WebGl2RenderingContext::STATIC_DRAW,
        );
        buffer
    }

    pub fn new(context: Rc<WebGl2RenderingContext>) -> Self {
        debug!("creating pixel pass");
        let vert = load_shader(
            &context,
            WebGl2RenderingContext::VERTEX_SHADER,
            include_str!("passes/pixel.vert.glsl"),
        );
        let frag = load_shader(
            &context,
            WebGl2RenderingContext::FRAGMENT_SHADER,
            include_str!("passes/pixel.frag.glsl"),
        );
        let program = init_program(&context, vert, frag);
        let position_buffer = Self::create_position_buffer(&&context);
        let a_vertex_position = context.get_attrib_location(&program, "a_vertex_position");
        let u_projection = context
            .get_uniform_location(&program, "u_projection")
            .unwrap();
        // let u_model_view = context
        //     .get_uniform_location(&program, "u_model_view")
        //     .unwrap();
        Self {
            context,
            program,
            position_buffer,
            a_vertex_position,
            u_projection,
            // u_model_view,
        }
    }

    pub fn render(&self) {
        let perspective = {
            let matrix = orthographic_gl(0.0, 1.0, 0.0, 1.0, -1.0, 1.0);
            matrix
        };

        let model_view = {
            let mut matrix = Mat4::identity();
            matrix.translate(&Vec3::new(0.0, 0.0, -6.0));
            matrix
        };

        self.context.use_program(Some(&self.program));

        {
            let num_components = 2;
            let buffer_type = WebGl2RenderingContext::FLOAT;
            let normalize = false;
            let stride = 0;
            let offset = 0;
            self.context.bind_buffer(
                WebGl2RenderingContext::ARRAY_BUFFER,
                Some(&self.position_buffer),
            );
            self.context.vertex_attrib_pointer_with_i32(
                self.a_vertex_position as u32,
                num_components,
                buffer_type,
                normalize,
                stride,
                offset,
            );
            self.context
                .enable_vertex_attrib_array(self.a_vertex_position as u32);
        }

        self.context.uniform_matrix4fv_with_f32_array(
            Some(&self.u_projection),
            false,
            cast_ref::<_, [f32; 16]>(&perspective),
        );
        // self.context.uniform_matrix4fv_with_f32_array(
        //     Some(&self.u_model_view),
        //     false,
        //     cast_ref::<_, [f32; 16]>(&model_view),
        // );

        {
            let offset = 0;
            let vertex_count = 4;
            self.context
                .draw_arrays(WebGl2RenderingContext::TRIANGLE_STRIP, offset, vertex_count);
        }
    }
}

struct SpritePass {
    context: Rc<WebGl2RenderingContext>,
    position_buffer: WebGlBuffer,
    program: WebGlProgram,
    // vertex_position attribute location
    a_vertex_position: i32,
    u_projection: WebGlUniformLocation,
}

impl SpritePass {
    fn create_position_buffer(context: &WebGl2RenderingContext) -> WebGlBuffer {
        let buffer = context.create_buffer().unwrap();
        context.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&buffer));
        let positions: &[f32] = &[1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0];
        context.buffer_data_with_array_buffer_view(
            WebGl2RenderingContext::ARRAY_BUFFER,
            &Float32Array::from(positions),
            WebGl2RenderingContext::STATIC_DRAW,
        );
        buffer
    }

    pub fn new(context: Rc<WebGl2RenderingContext>) -> Self {
        debug!("creating sprite pass");
        let vert = load_shader(
            &context,
            WebGl2RenderingContext::VERTEX_SHADER,
            include_str!("passes/sprite.vert.glsl"),
        );
        let frag = load_shader(
            &context,
            WebGl2RenderingContext::FRAGMENT_SHADER,
            include_str!("passes/sprite.frag.glsl"),
        );
        let program = init_program(&context, vert, frag);
        let position_buffer = Self::create_position_buffer(&&context);
        let a_vertex_position = context.get_attrib_location(&program, "a_vertex_position");
        let u_projection = context
            .get_uniform_location(&program, "u_projection")
            .unwrap();
        // let u_model_view = context
        //     .get_uniform_location(&program, "u_model_view")
        //     .unwrap();
        Self {
            context,
            program,
            position_buffer,
            a_vertex_position,
            u_projection,
            // u_model_view,
        }
    }

    pub fn render(&self) {
        let perspective = {
            let matrix = orthographic_gl(0.0, 1.0, 0.0, 1.0, -1.0, 1.0);
            matrix
        };

        let model_view = {
            let mut matrix = Mat4::identity();
            matrix.translate(&Vec3::new(0.0, 0.0, -6.0));
            matrix
        };

        self.context.use_program(Some(&self.program));

        {
            let num_components = 2;
            let buffer_type = WebGl2RenderingContext::FLOAT;
            let normalize = false;
            let stride = 0;
            let offset = 0;
            self.context.bind_buffer(
                WebGl2RenderingContext::ARRAY_BUFFER,
                Some(&self.position_buffer),
            );
            self.context.vertex_attrib_pointer_with_i32(
                self.a_vertex_position as u32,
                num_components,
                buffer_type,
                normalize,
                stride,
                offset,
            );
            self.context
                .enable_vertex_attrib_array(self.a_vertex_position as u32);
        }

        self.context.uniform_matrix4fv_with_f32_array(
            Some(&self.u_projection),
            false,
            cast_ref::<_, [f32; 16]>(&perspective),
        );
        // self.context.uniform_matrix4fv_with_f32_array(
        //     Some(&self.u_model_view),
        //     false,
        //     cast_ref::<_, [f32; 16]>(&model_view),
        // );

        {
            let offset = 0;
            let vertex_count = 4;
            self.context
                .draw_arrays(WebGl2RenderingContext::TRIANGLE_STRIP, offset, vertex_count);
        }
    }
}

fn init_program(
    context: &WebGl2RenderingContext,
    vertex_shader: WebGlShader,
    fragment_shader: WebGlShader,
) -> WebGlProgram {
    debug!("creating webgl program");
    let program = context.create_program().unwrap();

    context.attach_shader(&program, &vertex_shader);
    context.attach_shader(&program, &fragment_shader);
    context.link_program(&program);

    if !context
        .get_program_parameter(&program, WebGl2RenderingContext::LINK_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        panic!(
            "Unable to initialize the shader program: {}",
            context
                .get_program_info_log(&program)
                .unwrap_or_else(|| "(could not read log)".to_string()),
        );
    }

    program
}

fn load_shader(context: &WebGl2RenderingContext, shader_type: u32, source: &str) -> WebGlShader {
    debug!("creating webgl shader");
    let shader = context.create_shader(shader_type).unwrap();
    context.shader_source(&shader, source);
    context.compile_shader(&shader);

    if !context
        .get_shader_parameter(&shader, WebGl2RenderingContext::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        context.delete_shader(Some(&shader));
        panic!("failed to load shader");
    }

    shader
}
