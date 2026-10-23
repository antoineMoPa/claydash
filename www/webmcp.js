// WebMCP is an optional browser API. The editor runs normally when it is absent.
export async function registerClayDashTools(request) {
  const context = document.modelContext;
  if (!context?.registerTool) return;

  const noArguments = { type: 'object', properties: {}, additionalProperties: false };
  const vector2 = { type: 'array', items: { type: 'number' }, minItems: 2, maxItems: 2 };
  const vector3 = { type: 'array', items: { type: 'number' }, minItems: 3, maxItems: 3 };
  const sectionSchema = { type: 'object', properties: {
    station: { type: 'number' }, origin: vector3, right: vector3, up: vector3,
    points: { type: 'array', items: vector2, minItems: 3, maxItems: 4096 },
  }, required: ['station', 'origin', 'right', 'up', 'points'], additionalProperties: false };
  const sectionsSchema = { type: 'array', items: sectionSchema, minItems: 2, maxItems: 256 };
  const run = async (type, args = {}) => {
    try {
      const result = JSON.parse(await request(JSON.stringify({ type, ...args })));
      return { content: [{ type: 'text', text: JSON.stringify(result) }] };
    } catch (error) {
      return { isError: true, content: [{ type: 'text', text: String(error) }] };
    }
  };
  const parseResponse = response => {
    if (response.isError) throw new Error(response.content?.[0]?.text || 'Claydash request failed');
    return JSON.parse(response.content?.[0]?.text || '{}');
  };
  const nextFrame = () => new Promise(resolve => requestAnimationFrame(resolve));
  const settleViewport = async () => {
    await nextFrame();
    await nextFrame();
    await new Promise(resolve => setTimeout(resolve, 250));
  };
  const sourceViewport = (canvas, view, crop) => {
    const rect = crop || [view.viewport_origin[0], view.viewport_origin[1], view.viewport[0], view.viewport[1]];
    const [x, y, width, height] = rect.map(value => Math.round(value));
    if (width < 1 || height < 1 || x < 0 || y < 0 || x + width > canvas.width || y + height > canvas.height) {
      throw new Error('capture crop must fit inside the browser canvas');
    }
    return [x, y, width, height];
  };
  const snapshot = async canvas => {
    // WebGPU canvases can return black when drawn directly into a 2D context.
    const frame = new Image();
    frame.src = canvas.toDataURL('image/png');
    await frame.decode();
    return frame;
  };
  const labelAxis = direction => {
    const index = direction.reduce((best, value, i) => Math.abs(value) > Math.abs(direction[best]) ? i : best, 0);
    return `${direction[index] < 0 ? '−' : '+'}${['X', 'Y', 'Z'][index]}`;
  };
  const drawCaptureOverlay = (context, rect, view, title, sourceScale) => {
    const [x, y, width, height] = rect;
    const distance = Math.hypot(...view.position.map((value, i) => value - view.target[i]));
    const verticalSpan = view.projection_mode === 'Orthographic'
      ? 0.84 * distance : 2 * distance * Math.tan(Math.PI / 8);
    const pixelsPerUnit = view.viewport[1] / verticalSpan * sourceScale;
    const targetWidth = Math.min(120, width * 0.25);
    const idealUnits = targetWidth / pixelsPerUnit;
    const exponent = Math.floor(Math.log10(idealUnits));
    const magnitude = 10 ** exponent;
    const units = [1, 2, 5, 10].map(value => value * magnitude)
      .reduce((best, value) => Math.abs(value - idealUnits) < Math.abs(best - idealUnits) ? value : best);
    const barWidth = units * pixelsPerUnit;
    const forward = view.target.map((value, i) => (value - view.position[i]) / distance);
    const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    const unit = vector => vector.map(value => value / Math.hypot(...vector));
    const right = unit(cross(forward, view.up));
    const up = cross(right, forward);
    const small = width < 600;
    const inset = small ? 17 : 24;
    const font = small ? 13 : 15;
    context.save();
    context.font = `600 ${font}px system-ui, sans-serif`;
    context.textBaseline = 'middle';
    context.fillStyle = '#ffffff';
    context.strokeStyle = '#ffffff';
    context.lineWidth = 2;
    context.shadowColor = '#000000bb';
    context.shadowBlur = 5;
    context.textAlign = 'center';
    context.fillText(title, x + width / 2, y + inset + font / 2);
    const axisX = x + inset + 12, axisY = y + height - inset - 12;
    context.beginPath();
    context.moveTo(axisX, axisY - 30); context.lineTo(axisX, axisY);
    context.lineTo(axisX + 30, axisY); context.stroke();
    context.textAlign = 'left';
    context.fillText(labelAxis(right), axisX + 34, axisY);
    context.textAlign = 'center';
    context.fillText(labelAxis(up), axisX, axisY - 39);
    const barX = x + width - inset - barWidth, barY = y + height - inset - 8;
    context.beginPath();
    context.moveTo(barX, barY - 6); context.lineTo(barX, barY);
    context.lineTo(barX + barWidth, barY);
    context.lineTo(barX + barWidth, barY - 6); context.stroke();
    context.textAlign = 'center';
    context.fillText(`${Number(units.toPrecision(2))} ${units === 1 ? 'unit' : 'units'}`, barX + barWidth / 2, barY - 17);
    context.restore();
  };

  const tools = [
    {
      name: 'claydash_get_scene_summary',
      description: 'Read objects, selected objects, Gaussian point counts, and the current Claydash viewport camera.',
      inputSchema: noArguments,
      execute: () => run('get_scene_summary'),
    },
    {
      name: 'claydash_list_commands',
      description: 'List Claydash editor commands and their descriptions.',
      inputSchema: noArguments,
      execute: () => run('list_commands'),
    },
    {
      name: 'claydash_create_primitive',
      description: 'Add and select an undoable primitive in the current Claydash scene.',
      inputSchema: {
        type: 'object',
        properties: {
          kind: { type: 'string', enum: ['Box', 'Sphere', 'Cylinder', 'Torus', 'PolygonPrism', 'BezierCurve', 'Loft', 'SheetPanel', 'Text'] },
        },
        required: ['kind'],
        additionalProperties: false,
      },
      execute: ({ kind }) => run('create_primitive', { kind }),
    },
    {
      name: 'claydash_undo',
      description: 'Undo the last Claydash scene edit.',
      inputSchema: noArguments,
      execute: () => run('undo'),
    },
    {
      name: 'claydash_redo',
      description: 'Redo the last undone Claydash scene edit.',
      inputSchema: noArguments,
      execute: () => run('redo'),
    },
    {
      name: 'claydash_apply_mesh_actions',
      description: 'Create or edit an undoable direct triangle mesh. Actions: CreateMesh, CreateSectionMesh, SetMeshSections, AddMeshVertices, AddMeshFaces, SetMeshShading, ProjectMeshUvs, SetMeshUvs, SetMeshTexture. Section meshes use ordered local contour frames. SetMeshUvs supplies one [u,v] triple per face; SetMeshTexture attaches a base64 PNG after UVs are set.',
      inputSchema: {
        type: 'object',
        properties: {
          actions: {
            type: 'array', minItems: 1,
            items: { oneOf: [
              { type: 'object', properties: { type: { const: 'CreateMesh' }, name: { type: 'string' }, vertices: { type: 'array', items: { type: 'array', items: { type: 'number' }, minItems: 3, maxItems: 3 } }, faces: { type: 'array', items: { type: 'array', items: { type: 'integer', minimum: 0 }, minItems: 3, maxItems: 3 } }, shading: { enum: ['Auto', 'Flat', 'Smooth'] } }, required: ['type', 'vertices', 'faces'], additionalProperties: false },
              { type: 'object', properties: { type: { const: 'AddMeshVertices' }, id: { type: 'string' }, vertices: { type: 'array', items: { type: 'array', items: { type: 'number' }, minItems: 3, maxItems: 3 }, minItems: 1 } }, required: ['type', 'id', 'vertices'], additionalProperties: false },
              { type: 'object', properties: { type: { const: 'AddMeshFaces' }, id: { type: 'string' }, faces: { type: 'array', items: { type: 'array', items: { type: 'integer', minimum: 0 }, minItems: 3, maxItems: 3 }, minItems: 1 } }, required: ['type', 'id', 'faces'], additionalProperties: false },
              { type: 'object', properties: { type: { const: 'SetMeshShading' }, id: { type: 'string' }, shading: { enum: ['Auto', 'Flat', 'Smooth'] } }, required: ['type', 'id', 'shading'], additionalProperties: false },
              { type: 'object', properties: { type: { const: 'ProjectMeshUvs' }, id: { type: 'string' }, direction: { type: 'array', items: { type: 'number' }, minItems: 3, maxItems: 3 }, up: { type: 'array', items: { type: 'number' }, minItems: 3, maxItems: 3 }, scale: { type: 'array', items: { type: 'number' }, minItems: 2, maxItems: 2 }, offset: { type: 'array', items: { type: 'number' }, minItems: 2, maxItems: 2 } }, required: ['type', 'id', 'direction', 'up', 'scale', 'offset'], additionalProperties: false },
              { type: 'object', properties: { type: { const: 'SetMeshUvs' }, id: { type: 'string' }, uvs: { type: 'array', items: { type: 'array', items: { type: 'array', items: { type: 'number' }, minItems: 2, maxItems: 2 }, minItems: 3, maxItems: 3 } } }, required: ['type', 'id', 'uvs'], additionalProperties: false },
              { type: 'object', properties: { type: { const: 'SetMeshTexture' }, id: { type: 'string' }, png_base64: { type: 'string', description: 'Base64 PNG, square power-of-two 64–4096 px, at most 64 MiB decoded.' } }, required: ['type', 'id', 'png_base64'], additionalProperties: false },
              { type: 'object', properties: { type: { const: 'CreateSectionMesh' }, name: { type: 'string' }, sections: sectionsSchema, caps: { type: 'boolean' }, shading: { enum: ['Auto', 'Flat', 'Smooth'] } }, required: ['type', 'sections'], additionalProperties: false },
              { type: 'object', properties: { type: { const: 'SetMeshSections' }, id: { type: 'string' }, sections: sectionsSchema, caps: { type: 'boolean' } }, required: ['type', 'id', 'sections', 'caps'], additionalProperties: false },
            ] },
          },
          expected_revision: { type: 'integer', minimum: 0 },
        },
        required: ['actions'], additionalProperties: false,
      },
      execute: ({ actions, expected_revision }) => run('apply_mesh_actions', { actions, expected_revision }),
    },
    {
      name: 'claydash_get_mesh',
      description: 'Read an editable mesh by object UUID, including vertices, faces, per-corner UVs, shading and normals.',
      inputSchema: { type: 'object', properties: { id: { type: 'string' } }, required: ['id'], additionalProperties: false },
      execute: ({ id }) => run('get_mesh', { id }),
    },
    {
      name: 'claydash_set_view',
      description: 'Set the user viewport camera position, target, and up vector without editing scene geometry.',
      inputSchema: { type: 'object', properties: { position: vector3, target: vector3, up: vector3 }, required: ['position', 'target', 'up'], additionalProperties: false },
      execute: ({ position, target, up }) => run('set_view', { position, target, up }),
    },
    {
      name: 'claydash_preview_mesh_sections',
      description: 'Validate proposed ordered cross sections and preview their rings, triangle faces, and XY/XZ/YZ orthographic projections without editing the scene.',
      inputSchema: { type: 'object', properties: { sections: sectionsSchema, caps: { type: 'boolean' } }, required: ['sections'], additionalProperties: false },
      execute: ({ sections, caps }) => run('preview_mesh_sections', { sections, caps }),
    },
    {
      name: 'claydash_export_project',
      description: 'Return the current editable .claydash project as base64 bytes (up to 32 MiB decoded), with format and byte length. An oversized project returns an error; no data is truncated.',
      inputSchema: noArguments,
      execute: () => run('export_project'),
    },
    {
      name: 'claydash_open_project',
      description: 'Replace the current scene with a Claydash project supplied as base64 .claydash bytes (up to 32 MiB).',
      annotations: { consequential: true },
      inputSchema: { type: 'object', properties: { data_base64: { type: 'string' } }, required: ['data_base64'], additionalProperties: false },
      execute: ({ data_base64 }) => run('open_project', { data_base64 }),
    },
    {
      name: 'claydash_capture_viewport',
      description: 'Capture the viewport as a clean PNG, with view title, axis marker, and world-unit scale bar.',
      inputSchema: noArguments,
      execute: async () => {
        try {
          parseResponse(await run('set_capture_clean', { clean: true }));
          await settleViewport();
          const canvas = document.querySelector('canvas');
          if (!canvas) throw new Error('Claydash viewport is not ready');
          const view = parseResponse(await run('get_scene_summary')).view;
          const [sourceX, sourceY, width, height] = sourceViewport(canvas, view);
          const output = document.createElement('canvas');
          output.width = width; output.height = height;
          const context = output.getContext('2d');
          context.drawImage(await snapshot(canvas), sourceX, sourceY, width, height, 0, 0, width, height);
          drawCaptureOverlay(context, [0, 0, width, height], view, view.projection_mode, 1);
          const data = output.toDataURL('image/png').split(',')[1];
          return { content: [{ type: 'image', data, mimeType: 'image/png' }] };
        } catch (error) {
          return { isError: true, content: [{ type: 'text', text: String(error) }] };
        } finally {
          try { parseResponse(await run('set_capture_clean', { clean: false })); }
          catch (error) { console.warn('Could not restore Claydash viewport controls', error); }
        }
      },
    },
    {
      name: 'claydash_capture_multi_angle',
      description: 'Capture clean front, side, top, and three-quarter viewport views in one PNG contact sheet, each with an axis marker and world-unit scale bar. Restores the original camera and controls.',
      inputSchema: { type: 'object', properties: { crop: { type: 'array', items: { type: 'integer', minimum: 0 }, minItems: 4, maxItems: 4,
        description: 'Optional canvas-pixel [x,y,width,height] crop; width and height must be positive.' },
      views: { type: 'array', minItems: 4, maxItems: 4,
        items: { type: 'object', properties: { label: { type: 'string' }, position: vector3, target: vector3, up: vector3 },
          required: ['label', 'position', 'target', 'up'], additionalProperties: false } } }, additionalProperties: false },
      execute: async ({ views: requestedViews, crop } = {}) => {
        let original;
        try {
          original = parseResponse(await run('get_scene_summary')).view;
          parseResponse(await run('set_capture_clean', { clean: true }));
          const target = original.target;
          const distance = Math.max(1, Math.hypot(...original.position.map((value, i) => value - target[i])));
          const views = requestedViews || [
            { label: 'Front', position: [target[0], target[1], target[2] + distance], target, up: [0, 1, 0] },
            { label: 'Side', position: [target[0] + distance, target[1], target[2]], target, up: [0, 1, 0] },
            { label: 'Top', position: [target[0], target[1] + distance, target[2]], target, up: [0, 0, -1] },
            { label: 'Three-quarter', position: [target[0] + distance * 0.65, target[1] + distance * 0.45, target[2] + distance * 0.65], target, up: [0, 1, 0] },
          ];
          const sheet = document.createElement('canvas');
          sheet.width = 1024;
          sheet.height = 1024;
          const context = sheet.getContext('2d');
          const canvas = document.querySelector('canvas');
          if (!canvas) throw new Error('Claydash viewport is not ready');
          for (let index = 0; index < views.length; index++) {
            const { label, position, target: viewTarget, up } = views[index];
            parseResponse(await run('set_view', { position, target: viewTarget, up }));
            await settleViewport();
            const x = index % 2 * 512;
            const y = Math.floor(index / 2) * 512;
            // WebGPU canvases can return black when drawn directly into a 2D
            // context even though their PNG snapshot contains the rendered frame.
            const frame = await snapshot(canvas);
            context.fillStyle = '#202028';
            context.fillRect(x, y, 512, 512);
            const currentView = parseResponse(await run('get_scene_summary')).view;
            const [sourceX, sourceY, sourceWidth, sourceHeight] = sourceViewport(canvas, currentView, crop);
            const scale = Math.min(512 / sourceWidth, 512 / sourceHeight);
            const width = sourceWidth * scale;
            const height = sourceHeight * scale;
            context.drawImage(frame, sourceX, sourceY, sourceWidth, sourceHeight,
              x + (512 - width) / 2, y + (512 - height) / 2, width, height);
            drawCaptureOverlay(context,
              [x + (512 - width) / 2, y + (512 - height) / 2, width, height],
              currentView, label, scale);
          }
          return { content: [{ type: 'image', data: sheet.toDataURL('image/png').split(',')[1], mimeType: 'image/png' }] };
        } catch (error) {
          return { isError: true, content: [{ type: 'text', text: String(error) }] };
        } finally {
          if (original) {
            try { parseResponse(await run('set_view', { position: original.position, target: original.target, up: original.up })); }
            catch (error) { console.warn('Could not restore Claydash viewport camera', error); }
          }
          try { parseResponse(await run('set_capture_clean', { clean: false })); }
          catch (error) { console.warn('Could not restore Claydash viewport controls', error); }
        }
      },
    },
  ];

  for (const tool of tools) {
    try {
      await context.registerTool(tool);
    } catch (error) {
      console.warn(`Could not register ${tool.name} with WebMCP`, error);
    }
  }
}
