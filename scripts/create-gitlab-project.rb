g = Group.find(72)
u = User.admins.first
existing = Project.find_by_full_path("hecate/hecate-lampad-proxmox")
if existing
  puts "EXISTS:#{existing.id}"
else
  p = Projects::CreateService.new(u, {
    name: "hecate-lampad-proxmox",
    path: "hecate-lampad-proxmox",
    namespace_id: g.id,
    visibility_level: Gitlab::VisibilityLevel::PRIVATE,
    initialize_with_readme: false,
    default_branch: "master"
  }).execute
  if p.persisted?
    puts "CREATED:#{p.id}"
  else
    puts "ERROR:#{p.errors.full_messages.join(',')}"
  end
end