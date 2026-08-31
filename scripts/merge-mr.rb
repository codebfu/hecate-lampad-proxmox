def merge_mr(full_path, iid)
  project = Project.find_by_full_path(full_path)
  mr = project.merge_requests.find_by!(iid: iid)
  user = User.admins.first
  result = MergeRequests::MergeService.new(project: project, current_user: user).execute(mr)
  puts "#{full_path}!#{iid} => #{mr.reload.state} result=#{result.class}"
end

merge_mr("hecate/hecate", 1)