output "public_ip" {
  value = aws_instance.e2e.public_ip
}

output "ssh_user" {
  value = "fedora"
}

output "key_path" {
  value = local_sensitive_file.key.filename
}

output "ami_name" {
  value = data.aws_ami.fedora.name
}
