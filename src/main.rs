extern crate rscam;
extern crate winit;
extern crate png;

#[macro_use] extern crate vulkano;
extern crate vulkano_shaders;
extern crate vulkano_win;
extern crate structopt;
#[macro_use] extern crate structopt_derive;

use vulkano_win::VkSurfaceBuild;
use vulkano::sync::GpuFuture;
use vulkano::framebuffer::Subpass;
use vulkano::descriptor::descriptor_set::PersistentDescriptorSet;
use vulkano::framebuffer::Framebuffer;
use vulkano::buffer::{CpuAccessibleBuffer, CpuBufferPool};
use vulkano::image::Dimensions;
use vulkano::image::StorageImage;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::swapchain::acquire_next_image;
use vulkano::swapchain::Swapchain;
use vulkano::swapchain::SwapchainCreationError;
use vulkano::pipeline::viewport::Viewport;
use vulkano::device::Device;
use vulkano::command_buffer::DynamicState;
use vulkano::image::ImageUsage;

use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use std::str;
use std::time::{Instant, Duration};

use structopt::StructOpt;

const DEFAULT_DIMENSIONS: [u32; 2] = [2448/4, 2048/4];

mod cam;
mod demosaic;
mod rggb;
mod cli;
mod events;

mod shaders;

#[derive(Debug, Clone)]
struct Vertex { position: [f32; 2] }
impl_vertex!(Vertex, position);

#[repr(C)]
#[derive(Copy, Clone)]
struct PushConstant {
    aspect: [f32; 2],
    offset: [f32; 2],
    zoom: f32,
}

struct EngineState {
    recreate_swapchain: bool,
    lmb_pressed: bool,
    grid_on: bool,
    hist_on: bool,
    fps_on: bool,
    is_grey: bool,
    done: bool,
    hidpi: f64,
    dimensions: [f64; 2],
    init_coor: [f32; 2],
    mouse_coor: [f32; 2],
    old_offset: [f32; 2],
    resolution: [u32; 2],
    push_consts: PushConstant,
    pause: Arc<AtomicBool>,
    cam_mutex: Arc<Mutex<cam::FrameBuf>>,
    dyn_state: DynamicState,
    frame_ts: u64,
}

fn rgb2rgba(pix: &[u8; 3]) -> [u8; 4] {
    [pix[0], pix[1], pix[2], 255]
}

fn main() -> Result<(), Box<std::error::Error>> {
    let args = cli::Cli::from_args();

    eprintln!("Waiting for camera... ");
    let cam = cam::Cam::new(&args.camera)?;

    let resolution = cam.get_resolution();

    let mut dimensions = DEFAULT_DIMENSIONS;

    let extensions = vulkano_win::required_extensions();
    let instance = vulkano::instance::Instance::new(None, &extensions, None)
        .expect("failed to create instance");

    let physical = vulkano::instance::PhysicalDevice::enumerate(&instance)
                            .next().expect("no device available");
    eprintln!("Using device: {} (type: {:?})", physical.name(), physical.ty());

    let mut events_loop = winit::EventsLoop::new();
    let surface = winit::WindowBuilder::new()
        .with_title("Camera")
        .with_dimensions((dimensions[0], dimensions[1]).into())
        //.with_min_dimensions((dimensions[0], dimensions[1]).into())
        .with_decorations(true)
        //.with_fullscreen(Some(events_loop.get_primary_monitor()))
        .build_vk_surface(&events_loop, instance.clone())
        .expect("failed to build window");

    let queue = physical.queue_families().find(|&q|
        q.supports_graphics() && surface.is_supported(q).unwrap_or(false)
    ).expect("couldn't find a graphical queue family");
    let device_ext = vulkano::device::DeviceExtensions {
        khr_swapchain: true,
        .. vulkano::device::DeviceExtensions::none()
    };
    let (device, mut queues) = Device::new(
        physical, physical.supported_features(), &device_ext,
        [(queue, 0.5)].iter().cloned()
    ).expect("failed to create device");
    let queue = queues.next().unwrap();


    let pause = Arc::new(AtomicBool::new(false));
    let is_grey = cam.is_grey();
    let cam_mutex = cam.run_worker(pause.clone());




    let (mut swapchain, mut images) = {
        let caps = surface.capabilities(physical)
            .expect("failed to get surface capabilities");

        dimensions = caps.current_extent.unwrap_or(dimensions);
        let usage = caps.supported_usage_flags;
        let alpha = caps.supported_composite_alpha.iter().next().unwrap();
        let format = caps.supported_formats[0].0;

        Swapchain::new(
            device.clone(), surface.clone(), caps.min_image_count,
            format, dimensions, 1, usage, &queue,
            vulkano::swapchain::SurfaceTransform::Identity, alpha,
            args.mode, true, None
        ).expect("failed to create swapchain")
    };

    let vertex_buffer = CpuAccessibleBuffer::<[Vertex]>::from_iter(
        device.clone(), vulkano::buffer::BufferUsage::all(),
       [
            Vertex { position: [-1.0, -1.0 ] },
            Vertex { position: [-1.0,  1.0 ] },
            Vertex { position: [ 1.0, -1.0 ] },
            Vertex { position: [ 1.0,  1.0 ] },
       ].iter().cloned()
    ).expect("failed to create buffer");

    let mut grid = Vec::new();

    let w_f32 = (resolution[0] as f32)/2.;
    for i in (args.grid_step..resolution[0]).step_by(args.grid_step as usize) {
        let c = (i as f32)/w_f32 - 1.;
        grid.extend_from_slice(&[[c, -1.], [c, 1.]]);
    }

    let h_f32 = (resolution[1] as f32)/2.;
    for i in (args.grid_step..resolution[1]).step_by(args.grid_step as usize) {
        let c = (i as f32)/h_f32 - 1.;
        grid.extend_from_slice(&[[-1., c], [1., c]]);
    }

    let grid_vertex_buffer = CpuAccessibleBuffer::<[Vertex]>::from_iter(
        device.clone(), vulkano::buffer::BufferUsage::all(),
        grid.iter().map(|v| Vertex { position: [v[0], v[1]] } )
    ).expect("failed to create buffer");


    let vs = shaders::vs::Shader::load(device.clone())
        .expect("failed to create shader module");
    let fs = shaders::fs::Shader::load(device.clone())
        .expect("failed to create shader module");

    let vs2 = shaders::vs2::Shader::load(device.clone())
        .expect("vs2: failed to create shader module");
    let fs2 = shaders::fs2::Shader::load(device.clone())
        .expect("fs2: failed to create shader module");

    let vs3 = shaders::vs3::Shader::load(device.clone())
        .expect("vs2: failed to create shader module");
    let fs3 = shaders::fs3::Shader::load(device.clone())
        .expect("fs2: failed to create shader module");

    let renderpass = Arc::new(
        single_pass_renderpass!(device.clone(),
            attachments: {
                color: {
                    load: Clear,
                    store: Store,
                    format: swapchain.format(),
                    samples: 1,
                }
            },
            pass: {
                color: [color],
                depth_stencil: {}
            }
        ).unwrap()
    );

    let texture = StorageImage::with_usage(
        device.clone(),
        Dimensions::Dim2d { width: resolution[0], height: resolution[1] },
        vulkano::format::R8G8B8A8Srgb,
        ImageUsage {
            transfer_destination: true, sampled: true, ..ImageUsage::none()
        },
        Some(queue.family()),
    ).unwrap();


    let addr_mode = vulkano::sampler::SamplerAddressMode::ClampToBorder(
        vulkano::sampler::BorderColor::IntTransparentBlack
    );
    let sampler = vulkano::sampler::Sampler::new(
        device.clone(),
        vulkano::sampler::Filter::Nearest,
        vulkano::sampler::Filter::Linear,
        vulkano::sampler::MipmapMode::Nearest,
        addr_mode, addr_mode, addr_mode,
        0.0, 1.0, 0.0, 0.0
    ).unwrap();

    let pipeline = Arc::new(vulkano::pipeline::GraphicsPipeline::start()
        .vertex_input_single_buffer::<Vertex>()
        .vertex_shader(vs.main_entry_point(), ())
        .triangle_strip()
        .viewports_dynamic_scissors_irrelevant(1)
        .fragment_shader(fs.main_entry_point(), ())
        .blend_alpha_blending()
        .render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
        .build(device.clone())
        .expect("Failed to build main pipeline")
    );

    let grid_pipeline = Arc::new(vulkano::pipeline::GraphicsPipeline::start()
        .vertex_input_single_buffer::<Vertex>()
        .vertex_shader(vs2.main_entry_point(), ())
        .line_list()
        .viewports_dynamic_scissors_irrelevant(1)
        .fragment_shader(fs2.main_entry_point(), ())
        .render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
        .build(device.clone())
        .expect("Failed to build grid pipeline")
    );

    let hist_pipeline = Arc::new(vulkano::pipeline::GraphicsPipeline::start()
        .vertex_input_single_buffer::<Vertex>()
        .vertex_shader(vs3.main_entry_point(), ())
        .line_strip()
        .viewports_dynamic_scissors_irrelevant(1)
        .fragment_shader(fs3.main_entry_point(), ())
        .blend_alpha_blending()
        .render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
        .build(device.clone())
        .expect("Failed to build main pipeline")
    );

    let set = Arc::new(PersistentDescriptorSet::start(pipeline.clone(), 0)
        .add_sampled_image(texture.clone(), sampler.clone()).unwrap()
        .build().unwrap()
    );

    let mut framebuffers: Vec<Arc<Framebuffer<_,_>>> = images.iter()
        .map(|image|
            Arc::new(Framebuffer::start(renderpass.clone())
                 .add(image.clone()).unwrap()
                 .build().unwrap())
        ).collect::<Vec<Arc<Framebuffer<_,_>>>>();

    let prev_frame = Box::new(vulkano::sync::now(device.clone()));
    let mut previous_frame = prev_frame as Box<GpuFuture>;

    let hidpi = surface.window().get_hidpi_factor();
    let mut state = EngineState {
        recreate_swapchain: false,
        lmb_pressed: false,
        grid_on: false,
        hist_on: false,
        fps_on: false,
        is_grey: is_grey,
        done: false,
        hidpi: hidpi,
        dimensions: [dimensions[0] as f64, dimensions[1] as f64],
        init_coor: [0f32; 2],
        mouse_coor: [0f32; 2],
        old_offset: [0., 0.],
        resolution: resolution,
        push_consts: PushConstant {
            aspect: [1.0, 1.0], zoom: 1.0, offset: [0., 0.],
        },
        pause: pause,
        cam_mutex: cam_mutex,
        dyn_state: DynamicState {
            line_width: None,
            viewports: Some(vec![Viewport {
                origin: [0.0, 0.0],
                dimensions: [dimensions[0] as f32, dimensions[1] as f32],
                depth_range: 0.0 .. 1.0,
            }]),
            scissors: None,
        },
        frame_ts: 0u64,
    };

    let buf_pool = CpuBufferPool::upload(device.clone());
    let mut chunk = buf_pool.chunk(
        (0..resolution[0]*resolution[1]).map(|_| [0u8, 0, 0, 255]))
        .unwrap();

    let mut hist_vertices: Vec<Vertex> = (0..=255)
        .map(|x| Vertex { position: [(x as f32)/255., 0.] })
        .collect();

    let mut t = Instant::now();
    let mut fc = 0;
    loop {
        let dt = t.elapsed();
        if dt > Duration::from_secs(1) {
            if state.fps_on {
                let micros = dt.subsec_micros() as f32;
                let secs = dt.as_secs() as f32 + micros/1_000_000.;
                println!("fps: {:?}", (fc as f32)/secs);
            }

            t = Instant::now();
            fc = 0;
        }

        previous_frame.cleanup_finished();
        events_loop.poll_events(|event| events::handle(event, &mut state));

        if state.recreate_swapchain {
            let res = state.resolution;
            let default_dims = [
                (state.dimensions[0]*state.hidpi) as u32,
                (state.dimensions[1]*state.hidpi) as u32,
            ];
            let dims = surface.capabilities(physical)
                .expect("failed to get surface capabilities")
                .current_extent.unwrap_or(default_dims);

            match swapchain.recreate_with_dimension(dims) {
                Ok((new_swapchain, new_images)) => {
                    swapchain = new_swapchain;
                    images = new_images;
                },
                Err(SwapchainCreationError::UnsupportedDimensions) => {
                    continue;
                },
                Err(err) => panic!("{:?}", err)
            };

            let r_p1 = (dims[0] as f32)/(dims[1] as f32);
            let r_p2 = (res[0] as f32)/(res[1] as f32);

            state.push_consts.aspect = if r_p1 > r_p2 {
                [r_p2/r_p1, 1.]
            } else {
                [1.0, r_p1/r_p2]
            };

            framebuffers = images.iter().map(|image|
                Arc::new(Framebuffer::start(renderpass.clone())
                         .add(image.clone()).unwrap()
                         .build().unwrap())
                ).collect::<Vec<_>>();

            match &mut state.dyn_state.viewports {
                Some(v) if v.len() == 1 => {
                    v[0].dimensions = [dims[0] as f32, dims[1] as f32];
                },
                _ => panic!("unexpected viewports value"),
            };

            state.recreate_swapchain = false;
        }

        let next_img = acquire_next_image(swapchain.clone(), None);
        let (image_num, future) = match next_img {
            Ok(r) => r,
            Err(vulkano::swapchain::AcquireError::OutOfDate) => {
                state.recreate_swapchain = true;
                continue;
            },
            Err(err) => panic!("{:?}", err)
        };

        {
            let guard = state.cam_mutex.lock().unwrap();
            if guard.ts != state.frame_ts {
                state.frame_ts = guard.ts;
                chunk = buf_pool
                    .chunk(guard.buf.iter().map(rgb2rgba))
                    .unwrap();

                let hist_max = guard.hist.iter().cloned().max().unwrap() as f32;
                for (&val, vert) in guard.hist.iter().zip(hist_vertices.iter_mut()) {
                    vert.position[1] = 1.0 - (val as f32)/hist_max;
                }
            }
        };

        let mut cbb = AutoCommandBufferBuilder
            ::primary_one_time_submit(device.clone(), queue.family())
            .unwrap()
            .copy_buffer_to_image(chunk.clone(), texture.clone())
            .expect("Failed to copy data to texture")
            .begin_render_pass(
                framebuffers[image_num].clone(), false,
                vec![[0.0, 0.0, 0.0, 1.0].into()]).unwrap()
            .draw(
                pipeline.clone(),
                &state.dyn_state,
                vertex_buffer.clone(),
                set.clone(), state.push_consts,
            ).expect("Main pipeline draw fail");

        if state.grid_on {
            cbb = cbb
                .draw(
                    grid_pipeline.clone(),
                    &state.dyn_state,
                    grid_vertex_buffer.clone(),
                    (), state.push_consts,
                ).expect("grid pipeline draw fail");
        }

        if state.hist_on {
            let hist_vertices = CpuAccessibleBuffer::from_iter(
                device.clone(),
                vulkano::buffer::BufferUsage::all(),
                hist_vertices.iter().cloned()
            ).expect("failed to create buffer");

            let [w, h] = events::get_dims(&state);
            let dyn_state = DynamicState {
                line_width: None,
                viewports: Some(vec![Viewport {
                    origin: [0., 0.],
                    dimensions: [w, h],
                    depth_range: 0.0 .. 1.0,
                }]),
                scissors: None,
            };

            cbb = cbb.draw(
                hist_pipeline.clone(),
                &dyn_state,
                hist_vertices.clone(),
                set.clone(), ()
            ).unwrap()
        }

        let cb = cbb.end_render_pass().unwrap().build().unwrap();

        let future = previous_frame.join(future)
            .then_execute(queue.clone(), cb).unwrap()
            .then_swapchain_present(queue.clone(), swapchain.clone(), image_num)
            .then_signal_fence_and_flush().unwrap();
        previous_frame = Box::new(future) as Box<vulkano::sync::GpuFuture>;

        if state.done { return Ok(()); }

        fc += 1;
    }
}
