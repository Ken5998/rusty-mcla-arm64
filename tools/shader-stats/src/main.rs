//! Builds the city pipeline (vs_main + fs_main, as render/mod.rs does) from each WGSL file given,
//! on whatever Vulkan driver is there: with Turnip on Mesa's drm-shim and IR3_SHADER_DEBUG=disasm
//! the Adreno compiler prints what it made of it.
fn main() {
    env_logger::init();
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..Default::default() });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("no Vulkan adapter");
    eprintln!("adapter: {:?}", adapter.get_info());
    let (device, _queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor { required_features: adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC, required_limits: adapter.limits(), ..Default::default() },
        None,
    ))
    .expect("no device");
    let attrs = [
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 24, shader_location: 2 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 32, shader_location: 3 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm8x4, offset: 40, shader_location: 4 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm8x4, offset: 44, shader_location: 5 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 48, shader_location: 8 },
    ];
    let layout = wgpu::VertexBufferLayout { array_stride: 56, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs };
    for path in std::env::args().skip(1) {
        eprintln!("=== {path}");
        println!("=== {path}");
        let src = std::fs::read_to_string(&path).expect("read");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(&path), source: wgpu::ShaderSource::Wgsl(src.into()) });
        let t = std::time::Instant::now();
        let _p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("city opaque"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), buffers: &[layout.clone()], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { front_face: wgpu::FrontFace::Cw, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        device.poll(wgpu::Maintain::Wait);
        eprintln!("pipeline built in {:.1} s", t.elapsed().as_secs_f32());
    }
}
