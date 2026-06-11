terraform {
  required_version = ">= 1.6"
  required_providers {
    aws   = { source = "hashicorp/aws", version = "~> 5.0" }
    tls   = { source = "hashicorp/tls", version = "~> 4.0" }
    http  = { source = "hashicorp/http", version = "~> 3.0" }
    local = { source = "hashicorp/local", version = "~> 2.0" }
  }
}

provider "aws" {
  region = var.region
}

# Latest STABLE Fedora Cloud Base AMI, owned by the Fedora Project.
#
# The name glob pins the major version and anchors the build token to a
# leading digit (`-44-2*`). Prerelease/Beta builds are named
# `...-44-Prerelease-*`, so requiring a digit after the version drops them.
# Pinning the version also stops most_recent from selecting an older release
# whose same-day build happens to sort newer (the F43-vs-F44 tie we hit).
data "aws_ami" "fedora" {
  most_recent = true
  owners      = ["125523088429"]
  filter {
    name   = "name"
    values = ["Fedora-Cloud-Base-AmazonEC2.${local.ami_arch}-${var.fedora_version}-2*"]
  }
  filter {
    name   = "architecture"
    values = [var.architecture]
  }
  filter {
    name   = "root-device-type"
    values = ["ebs"]
  }
  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
  filter {
    name   = "state"
    values = ["available"]
  }
}

# Latest official Red Hat RHEL AMI, owned by Red Hat (309956199498).
# Name family is `RHEL-<ver>_HVM-<date>-<arch>-0-Hourly2-GP3` for early builds
# and `RHEL-<ver>_HVM_GA-<date>-...` for point-release GA builds, e.g.
# `RHEL-10.2.0_HVM_GA-20260521-x86_64-0-Hourly2-GP3`. The glob uses `_HVM*`
# (NOT `_HVM-`) so it matches BOTH the `_HVM-` and `_HVM_GA-` families;
# anchoring to `_HVM-` alone drops the GA point releases (the newest images) and
# most_recent would then pick a stale minor. Verified against rhel_images.json:
# `_HVM*` matches 17/17 Hourly2 x86_64 RHEL-10 images, `_HVM-` only 14/17.
# Hourly2 images are PAYG (no BYOS subscription), so BaseOS/AppStream repos are
# enabled out of the box.
data "aws_ami" "rhel" {
  most_recent = true
  owners      = ["309956199498"]
  filter {
    name   = "name"
    values = ["RHEL-${var.rhel_version}*_HVM*-${var.architecture}-*-Hourly2-GP3"]
  }
  filter {
    name   = "architecture"
    values = [var.architecture]
  }
  filter {
    name   = "root-device-type"
    values = ["ebs"]
  }
  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
  filter {
    name   = "state"
    values = ["available"]
  }
}

data "http" "myip" {
  url = "https://checkip.amazonaws.com/"
}

locals {
  # Image names use aarch64/x86_64; the EC2 architecture filter uses arm64/x86_64.
  ami_arch   = var.architecture == "arm64" ? "aarch64" : var.architecture
  fedora_ami = coalesce(var.fedora_ami_id, data.aws_ami.fedora.id)
  rhel_ami   = coalesce(var.rhel_ami_id, data.aws_ami.rhel.id)
  ami_id     = var.os == "rhel10" ? local.rhel_ami : local.fedora_ami
  ami_name   = var.os == "rhel10" ? data.aws_ami.rhel.name : data.aws_ami.fedora.name
  ssh_user   = var.os == "rhel10" ? "ec2-user" : "fedora"
  ssh_cidr   = coalesce(var.allowed_ssh_cidr, "${chomp(data.http.myip.response_body)}/32")
  tags = {
    Project   = "fez"
    Purpose   = "e2e"
    ManagedBy = "terraform"
  }
}

resource "tls_private_key" "e2e" {
  algorithm = "ED25519"
}

resource "local_sensitive_file" "key" {
  filename        = var.key_path
  content         = tls_private_key.e2e.private_key_openssh
  file_permission = "0600"
}

resource "aws_key_pair" "e2e" {
  key_name_prefix = "fez-e2e-"
  public_key      = tls_private_key.e2e.public_key_openssh
}

resource "aws_security_group" "e2e" {
  name_prefix = "fez-e2e-"
  description = "fez e2e ephemeral; SSH from caller only"
  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = [local.ssh_cidr]
  }
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
  tags = local.tags
}

resource "aws_instance" "e2e" {
  ami                    = local.ami_id
  instance_type          = var.instance_type
  key_name               = aws_key_pair.e2e.key_name
  vpc_security_group_ids = [aws_security_group.e2e.id]
  user_data = templatefile("${path.module}/user_data.sh", {
    login_user = local.ssh_user
  })
  tags = merge(local.tags, { Name = "fez-e2e" })

  lifecycle {
    precondition {
      # Only the Fedora auto-selection can drift onto a prerelease build; the
      # RHEL Hourly2 glob already excludes betas. Skip the check when a pin or
      # a non-fedora OS is in play.
      condition     = var.os != "fedora" || var.fedora_ami_id != null || !can(regex("(?i)prerelease|beta|rawhide|eln|rc", data.aws_ami.fedora.name))
      error_message = "Auto-selected Fedora AMI '${data.aws_ami.fedora.name}' looks like a pre-release; pin var.fedora_ami_id."
    }
  }
}
