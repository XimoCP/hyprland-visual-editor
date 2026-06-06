-- @Title: Energetic
-- @Icon: flame
-- @Color: #e11d48
-- @Tag: BOLD
-- @Desc: Maximum visual impact. Exaggerated rebounds (56%) and backward exits.

-- ------------------------------------------------------
-- ▄▀█ █▄░█ █ █▀▄▀█ ▄▀█ ▀█▀ █ █▀█ █▄░█
-- █▀█ █░▀█ █ █░▀░█ █▀█ ░█░ █ █▄█ █░▀█

-- ------------------------------------------------------
-- Optimized by NVL: Remapped high impact curve
-- to the input to avoid visual glitches when closing.
-- ------------------------------------------------------

---@diagnostic disable: undefined-global

hl.config({
    animations = {
        workspace_wraparound = true,
        enabled = true
    }
})

hl.curve("nitro", { type = "bezier", points = { { 0.34, 1.56 }, { 0.64, 1 } } })
hl.curve("recoil", { type = "bezier", points = { { 0.36, 0 }, { 0.66, -0.56 } } })
hl.curve("crazy", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.05 } } })



hl.animation({ leaf = "windowsIn", enabled = true, speed = 6, bezier = "nitro", style = "slide" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 5, bezier = "recoil", style = "slide" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 5, bezier = "crazy", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 3, bezier = "nitro" })
hl.animation({ leaf = "layers", enabled = true, speed = 4, bezier = "nitro", style = "popin" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 5, bezier = "nitro", style = "slide" })
