user = User.admins.first
["hecate/hecate", "hecate/hecate-lampad-core", "hecate/hecate-lampad-linux"].each do |path|
  project = Project.find_by_full_path(path)
  member = project.team.find_member(user.id)
  if member
    puts "#{path} existing access=#{member.access_level}"
    member.update!(access_level: Gitlab::Access::OWNER) if member.access_level < Gitlab::Access::OWNER
  else
    project.add_owner(user)
    puts "#{path} added owner"
  end
  puts "#{path} can_push=#{user.can?(:push_code, project)}"
end