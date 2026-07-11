-- @Title: Rebound
-- @Icon: trending-up
-- @Color: #fbbf24
-- @Tag: BOUNCY
-- @Desc: Vertical spring physics. The windows fall and bounce when opened.
-- Converted from 12_rebote.conf
-- =====================================

-- ======================================================
-- Rebound - Configuración de Animación en Lua
-- Versión optimizada para Hyprland v0.55+
-- ======================================================

-- 1. Activación Global
hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

-- 2. Definición de Curvas de Física (Bézier)
hl.curve("spring", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.1 } } })
hl.curve("crouch", { type = "bezier", points = { { 0.1, -0.1 }, { 0.1, 1.0 } } })

-- 3. Animaciones para Ventanas (Efecto Gravedad / Rebote)
hl.animation({ leaf = "windowsIn", enabled = true, speed = 5, bezier = "spring", style = "popin 80%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 5, bezier = "crouch", style = "popin 80%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 5, bezier = "spring", style = "slide" })

-- 4. Animaciones de Desvanecimiento (Fades y Capas)
hl.animation({ leaf = "fade", enabled = true, speed = 3, bezier = "default" })
hl.animation({ leaf = "fadeLayersIn", enabled = true, speed = 3, bezier = "default" })
hl.animation({ leaf = "layers", enabled = true, speed = 4, bezier = "spring", style = "popin" })

-- 5. Áreas de Trabajo / Desktops (Efecto Pogo Vertical)
hl.animation({ leaf = "workspaces", enabled = true, speed = 5, bezier = "spring", style = "slidevert" })
