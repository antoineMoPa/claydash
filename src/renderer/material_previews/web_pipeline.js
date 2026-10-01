// wgpu exposes device/buffer/texture handles, but not async pipeline creation.
// Keep this bridge limited to preview compilation and its single draw call.
const bufferBindings = [0, 1, 2, 3, 4, 5, 6, 8, 13];

export async function compilePreview(device, source, constants) {
    // Mirrors the scene binding ABI in renderer/initialization.rs.
    const entries = bufferBindings.map(binding => ({
        binding, visibility: GPUShaderStage.FRAGMENT,
        buffer: { type: binding === 0 ? 'uniform' : 'read-only-storage' },
    }));
    entries.push(
        { binding: 9, visibility: GPUShaderStage.FRAGMENT, texture: { viewDimension: '3d' } },
        { binding: 10, visibility: GPUShaderStage.FRAGMENT, sampler: { type: 'filtering' } },
        { binding: 11, visibility: GPUShaderStage.FRAGMENT, texture: { viewDimension: '2d-array' } },
        { binding: 12, visibility: GPUShaderStage.FRAGMENT, sampler: { type: 'filtering' } },
    );
    const layout = device.createBindGroupLayout({ entries });
    const module = device.createShaderModule({ label: 'material preview shader', code: source });
    const pipeline = await device.createRenderPipelineAsync({
        label: 'material preview pipeline',
        layout: device.createPipelineLayout({ bindGroupLayouts: [layout] }),
        vertex: { module, entryPoint: 'vs_main' },
        fragment: {
            module, entryPoint: 'fs_main', constants: JSON.parse(constants),
            targets: [{ format: 'rgba8unorm-srgb' }],
        },
    });
    const sampler = device.createSampler({ minFilter: 'linear', magFilter: 'linear' });
    return { pipeline, layout, sampler };
}

export function drawPreview(device, compiled, buffers, lattice, images, view) {
    const entries = bufferBindings.map((binding, index) => ({
        binding, resource: { buffer: buffers[index] },
    }));
    entries.push(
        { binding: 9, resource: lattice.createView() },
        { binding: 10, resource: compiled.sampler },
        { binding: 11, resource: images.createView({ dimension: '2d-array' }) },
        { binding: 12, resource: compiled.sampler },
    );
    const bindGroup = device.createBindGroup({ layout: compiled.layout, entries });
    const encoder = device.createCommandEncoder({ label: 'material preview encoder' });
    const pass = encoder.beginRenderPass({
        label: 'material preview sphere',
        colorAttachments: [{ view, loadOp: 'clear', storeOp: 'store', clearValue: [0, 0, 0, 0] }],
    });
    pass.setPipeline(compiled.pipeline);
    pass.setBindGroup(0, bindGroup);
    pass.draw(3);
    pass.end();
    device.queue.submit([encoder.finish()]);
}
