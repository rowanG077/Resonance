---
issues: []
prs: []
no-links: true
---

# Changed

Replace Bevy's PBR and sprite rendering with a native renderer that reduces per-frame CPU work through batched uploads, shared material records and reuse of prepared scene resources.

Release cached GPU textures after their assets retire, and release obsolete render attachments when resolution or anti-aliasing changes.

# Removed

The OpenGL graphics backend. Use Vulkan, DirectX 12 or Metal; a saved OpenGL choice now selects the backend automatically.
