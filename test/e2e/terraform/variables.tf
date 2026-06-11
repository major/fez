variable "region" {
  type    = string
  default = "us-east-2"
}

variable "instance_type" {
  type    = string
  default = "t3.micro"
}

variable "architecture" {
  type    = string
  default = "x86_64"
}

# Major Fedora release to pin. Keeps most_recent from picking an older
# release that happens to have a newer same-day build (F43 vs F44 tie).
variable "fedora_version" {
  type    = string
  default = "44"
}

variable "fedora_ami_id" {
  type    = string
  default = null
}

variable "allowed_ssh_cidr" {
  type    = string
  default = null
}

variable "key_path" {
  type    = string
  default = "fez-e2e-key"
}

# Which OSes to provision. Terraform's own DAG parallelizes the per-OS
# instances (aws_instance.e2e for_each = var.oses), so the runner does a single
# apply instead of fanning out N terraform dirs. Each element selects an AMI
# family and SSH login user (fedora vs ec2-user) via the maps in main.tf.
variable "oses" {
  type    = set(string)
  default = ["fedora"]
  validation {
    condition     = length(var.oses) > 0 && alltrue([for o in var.oses : contains(["fedora", "rhel10"], o)])
    error_message = "var.oses must be a non-empty subset of: fedora, rhel10."
  }
}

# Hard cap (seconds) on the per-host readiness remote-exec provisioner. A host
# whose cloud-init never writes /var/lib/fez-e2e-ready taints its instance and
# fails the apply for that host only; siblings still come up. The runner
# tolerates the non-zero apply and derives survivors from the output maps.
variable "ready_timeout_seconds" {
  type    = number
  default = 300
}

# Major RHEL release to match in the official Red Hat AMI name glob.
variable "rhel_version" {
  type    = string
  default = "10"
}

# Optional pin for an exact RHEL AMI id, bypassing the data-source lookup.
variable "rhel_ami_id" {
  type    = string
  default = null
}
