user = User.admins.first
["hecate/hecate", "hecate/hecate-lampad-core"].each do |path|
  project = Project.find_by_full_path(path)
  mr = project.merge_requests.find_by!(iid: 1)
  source_sha = mr.diff_head_sha
  target = project.repository.find_branch("master")
  puts "#{path} master=#{target.target} source=#{source_sha}"
  # Create merge commit via MergeRequests::CreatePipelineService? Use MergeToRef then
  # Direct FF if possible
  begin
    project.repository.merge(user, source_sha, "master", "Merge feature/proxmox-console-helper")
    puts "#{path} repository.merge done master=#{project.repository.find_branch('master').target}"
    mr.update!(state_id: 3, merge_commit_sha: project.repository.find_branch("master").target)
    puts "#{path} mr marked merged state=#{mr.reload.state}"
  rescue => e
    puts "#{path} ERR #{e.class}: #{e.message}"
  end
end