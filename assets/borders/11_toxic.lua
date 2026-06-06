-- @Title: Toxic
-- @Icon: biohazard
-- @Color: #39ff14
-- @Tag: ACID
-- @Desc: Intense radioactive green with toxic flow effect.
-- Converted from 11_toxic.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { "rgba(39ff14ff)", "rgba(0c1017ff)", "rgba(39ff14ff)" },
                angle = 30
            },
            inactive_border = "rgba(022c0b55)"
        },
        border_size = 2
    }
})

hl.curve("nv_toxic", { type = "bezier", points = { { 0.4, 0 }, { 0.2, 1 } } })
hl.animation({ leaf = "borderangle", enabled = true, speed = 40, bezier = "nv_toxic", style = "loop" })
