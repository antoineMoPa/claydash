---
name: claydash-basics
description: Choose primitives, materials, and render modes when creating or editing a Claydash scene through MCP.
---

# Claydash basics

- Start with `get_scene_summary`; use `get_action_schema` for the specific action and primitive you need. Tool schemas are the authority for arguments and defaults.
- Prefer native primitives for editable solids, SheetPanel for cut or bent sheet parts, section meshes for changing cross-sections, and direct meshes for explicit triangle control. Keep distinct physical parts independent.
- Settle shape and proportions in simple shading before judging materials in full-material captures. Reuse material assets across matching parts; use custom WGSL when a procedural surface is needed.
- Viewport representation and exported geometry are separate choices. Keep an editable `.claydash` source as well as any requested GLB; verify the export job completes before delivering its path.
