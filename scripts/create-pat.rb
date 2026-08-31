u = User.admins.first
token = u.personal_access_tokens.create!(
  name: "hecate-bootstrap-proxmox",
  scopes: ["api", "write_repository", "read_repository"],
  expires_at: 2.days.from_now
)
puts "TOKEN:#{token.token}"