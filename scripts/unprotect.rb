def unprotect_and_note(full_path)
  project = Project.find_by_full_path(full_path)
  pb = project.protected_branches.find_by(name: "master")
  if pb
    puts "#{full_path} before push_access=#{pb.push_access_levels.map(&:access_level)} merge_access=#{pb.merge_access_levels.map(&:access_level)}"
    pb.destroy!
    puts "#{full_path} master unprotected"
  else
    puts "#{full_path} master not protected"
  end
end

unprotect_and_note("hecate/hecate")
unprotect_and_note("hecate/hecate-lampad-core")
unprotect_and_note("hecate/hecate-lampad-linux")