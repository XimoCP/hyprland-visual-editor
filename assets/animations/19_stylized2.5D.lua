-- @Title: Stylized 2.5D
-- @Icon: palette
-- @Color: #fde047
-- @Tag: ARTISTIC
-- @Desc: Animación tradicional 2.5D. Efecto "Squash & Stretch" con anticipación al entrar y cortes rápidos al salir.

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

-- 1. Curvas de físicas tradicionales
hl.curve("anticipation", { type = "bezier", points = { { 0.4, -0.3 }, { 0.2, 1.15 } } })
hl.curve("keyframe_out", { type = "bezier", points = { { 1.0, 0.0 }, { 1.0, 1.0 } } })
hl.curve("squash", { type = "bezier", points = { { 0.2, 1.2 }, { 0.3, 1.0 } } })

-- 2. Gestión de Ventanas
hl.animation({ leaf = "windowsIn", enabled = true, speed = 4.5, bezier = "anticipation", style = "popin 70%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 1.5, bezier = "keyframe_out", style = "popin 95%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 4.0, bezier = "squash", style = "slide" })

-- 3. Transiciones de Entorno y Capas (Layers)
hl.animation({ leaf = "fade", enabled = true, speed = 3, bezier = "squash" })
hl.animation({ leaf = "fadeDim", enabled = true, speed = 3, bezier = "squash" })
hl.animation({ leaf = "layersIn", enabled = true, speed = 4, bezier = "anticipation", style = "slide right" })
hl.animation({ leaf = "layersOut", enabled = true, speed = 2, bezier = "keyframe_out", style = "slide right" })

-- 4. Espacios de trabajo y Scratchpads
hl.animation({ leaf = "workspaces", enabled = true, speed = 5, bezier = "anticipation", style = "slidefade 15%" })
hl.animation({ leaf = "specialWorkspace", enabled = true, speed = 4.5, bezier = "anticipation", style = "slidefadevert 20%" })
