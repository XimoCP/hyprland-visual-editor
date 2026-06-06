-- @Title: Subtle
-- @Icon: sparkles
-- @Color: #94a3b8
-- @Tag: PRO
-- @Desc: Zero distractions. Absolute smoothness without bounces or sudden movements.


------------------------------------------------------
-- ▄▀█ █▄░█ █ █▀▄▀█ ▄▀█ ▀█▀ █ █▀█ █▄░█
-- █▀█ █░▀█ █ █░▀░█ █▀█ ░█░ █ █▄█ █░▀█
--

------------------------------------------------------
---@diagnostic disable: undefined-global

hl.config({
    animations = {
        workspace_wraparound = true,
        enabled = true
    }
})

hl.curve("soft", { type = "bezier", points = { { 0.25, 1 }, { 0.5, 1 } } })
hl.curve("focus", { type = "bezier", points = { { 0.16, 1 }, { 0.3, 1 } } })


hl.animation({ leaf = "windowsIn", enabled = true, speed = 5, bezier = "soft", style = "popin 95%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 4, bezier = "soft", style = "popin 98%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 5, bezier = "soft", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 4, bezier = "soft" })
hl.animation({ leaf = "layers", enabled = true, speed = 4, bezier = "soft", style = "popin" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 5, bezier = "soft", style = "slidefade 5%" })
