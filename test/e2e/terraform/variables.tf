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
