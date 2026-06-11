# Per-OS maps, keyed by OS. The runner does one `terraform output -json` and
# parses these. Only OSes whose instance came up AND passed the readiness
# provisioner appear here; an OS that failed readiness taints its instance, so
# the apply is non-zero and that key is absent. The runner treats requested-but-
# absent keys as infra failures and proceeds with the survivors present here.
output "public_ips" {
  value = { for os, inst in aws_instance.e2e : os => inst.public_ip }
}

output "ssh_users" {
  value = { for os in keys(aws_instance.e2e) : os => local.ssh_user[os] }
}

output "ami_names" {
  value = { for os in keys(aws_instance.e2e) : os => local.ami_name[os] }
}

# Single shared key for every host.
output "key_path" {
  value = local_sensitive_file.key.filename
}
