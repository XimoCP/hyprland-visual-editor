-- @Title: Organic
-- @Icon: leaf
-- @Color: #a3e635
-- @Tag: NATURE
-- @Desc: "Sprout" movement. Smooth vertical growth and slow fades.

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("sprout", { type = "bezier", points = { { 0.25, 0.8 }, { 0.25, 1.0 } } })
hl.curve("wind", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.05 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 3, bezier = "sprout", style = "popin 85%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 3, bezier = "sprout", style = "popin 95%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 3, bezier = "wind", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 3, bezier = "sprout" })
hl.animation({ leaf = "layers", enabled = true, speed = 3, bezier = "sprout", style = "popin" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 4, bezier = "sprout", style = "slidevert" })
