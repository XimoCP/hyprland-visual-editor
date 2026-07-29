-- @Title: Rebound
-- @Icon: trending-up
-- @Color: #fbbf24
-- @Tag: BOUNCY
-- @Desc: Real spring physics. Windows fall, bounce and settle with proper physics.
-- =====================================

-- ======================================================
-- Rebound v2 — Spring curves reales + leafs nuevos
-- ======================================================

-- === CURVAS ===

-- Spring curve real: masa=1, rigidez=75, amortiguación=12
-- Los ventanas caen, rebotan y se asientan con física real de resorte
-- stiffness 75 = rebote rápido, dampening 12 = rebote visible
hl.curve("spring", { type = "spring", mass = 1, stiffness = 75, dampening = 12 })

-- Spring suave para el cierre — rebote más ligero, más suave
hl.curve("crouch", { type = "spring", mass = 1, stiffness = 50, dampening = 18 })

-- === ANIMACIONES ===

-- Ventanas — popin con spring bounce real
hl.animation({ leaf = "windowsIn", enabled = true, speed = 5, spring = "spring", style = "popin 80%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 5, spring = "crouch", style = "popin 80%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 5, spring = "spring", style = "slide" })

-- === FADES ===

-- Fade principal
hl.animation({ leaf = "fade", enabled = true, speed = 3, bezier = "default" })

-- Fade de popups — los menús y tooltips entran/salen suavemente
hl.animation({ leaf = "fadePopupsIn", enabled = true, speed = 3, bezier = "default" })
hl.animation({ leaf = "fadePopupsOut", enabled = true, speed = 3, bezier = "default" })

-- Fade de capas (layers/notifications)
hl.animation({ leaf = "fadeLayersIn", enabled = true, speed = 3, bezier = "default" })
hl.animation({ leaf = "fadeLayersOut", enabled = true, speed = 3, bezier = "default" })

-- Fade del shadow — transición suave de la sombra al cambiar ventana
hl.animation({ leaf = "fadeShadow", enabled = true, speed = 4, bezier = "default" })

-- Fade del glow — transición suave del brillo
hl.animation({ leaf = "fadeGlow", enabled = true, speed = 4, bezier = "default" })

-- Dim de ventanas inactivas — se oscurecen con easing
hl.animation({ leaf = "fadeDim", enabled = true, speed = 4, bezier = "default" })

-- Capas con spring popin
hl.animation({ leaf = "layers", enabled = true, speed = 4, spring = "spring", style = "popin" })

-- === WORKSPACES ===

-- Workspace switching con spring slide vert
hl.animation({ leaf = "workspaces", enabled = true, speed = 5, spring = "spring", style = "slidevert" })
hl.animation({ leaf = "specialWorkspace", enabled = true, speed = 5, spring = "spring", style = "slidevert" })

-- Zoom factor — al cambiar workspace, zoom sutil
hl.animation({ leaf = "zoomFactor", enabled = true, speed = 6, bezier = "default" })

-- Monitor added — animación al conectar/desconectar pantalla
hl.animation({ leaf = "monitorAdded", enabled = true, speed = 5, bezier = "default" })

-- Border angle rotativo sutil
hl.animation({ leaf = "borderangle", enabled = true, speed = 30, bezier = "default", style = "loop" })
hl.animation({ leaf = "shadowangle", enabled = true, speed = 30, bezier = "default", style = "loop" })
hl.animation({ leaf = "glowangle", enabled = true, speed = 30, bezier = "default", style = "loop" })
