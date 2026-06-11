output "public_ip" {
  value = aws_instance.e2e.public_ip
}

output "ssh_user" {
  value = local.ssh_user
}

output "key_path" {
  value = local_sensitive_file.key.filename
}

output "ami_name" {
  value = local.ami_name
}
