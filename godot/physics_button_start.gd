extends Button

func _on_pressed():
	var terrain = $/root/Node3D/MeshInstance3D
	terrain.start_physics()
