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

# Which OS to provision: "fedora" or "rhel10". Selects the AMI data source
# and the SSH login user (fedora vs ec2-user) in main.tf.
variable "os" {
  type    = string
  default = "fedora"
  validation {
    condition     = contains(["fedora", "rhel10"], var.os)
    error_message = "var.os must be one of: fedora, rhel10."
  }
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
