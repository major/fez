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
#
# Red Hat publishes two families per minor: interim refreshes named
# `RHEL-<ver>_HVM-<date>-...` and point-release GA builds named
# `RHEL-<ver>_HVM_GA-<date>-...`, e.g. `RHEL-10.2.0_HVM_GA-20260521-...`. We
# pin to **GA only** (`_HVM_GA-`) for two reasons:
#
#  1. Correct minor selection. `most_recent` sorts by CreationDate, NOT by the
#     version in the name. A refreshed base image (e.g. `10.0.0_HVM-20260522`)
#     gets registered AFTER the newest GA (`10.2.0_HVM_GA-20260521`), so a glob
#     that matches both families lets most_recent pick the OLDER minor (10.0.0)
#     purely because its rebuild is newer by date. This is the same "same-day
#     build sorts newer" trap the Fedora data source warns about. Within the GA
#     family alone, CreationDate and version order agree (10.2.0 > 10.1.0 >
#     10.0.0), so most_recent yields the latest GA point release. As 10.3 GAs
#     later, its newer date keeps this current with no edit.
#  2. GA images are the released surface we want to test, not interim rebuilds.
#
# Hourly2 images are PAYG (no BYOS subscription), so BaseOS/AppStream repos are
# enabled out of the box.
data "aws_ami" "rhel" {
  most_recent = true
  owners      = ["309956199498"]
  filter {
    name   = "name"
    values = ["RHEL-${var.rhel_version}*_HVM_GA-*-${var.architecture}-*-Hourly2-GP3"]
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

  # Per-OS lookup maps. Both AMI families are always resolved (the data sources
  # are cheap and unconditional), but only the OSes in var.oses get instances.
  ami_id   = { fedora = local.fedora_ami, rhel10 = local.rhel_ami }
  ami_name = { fedora = data.aws_ami.fedora.name, rhel10 = data.aws_ami.rhel.name }
  ssh_user = { fedora = "fedora", rhel10 = "ec2-user" }

  ssh_cidr = coalesce(var.allowed_ssh_cidr, "${chomp(data.http.myip.response_body)}/32")
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

# One instance per requested OS. Terraform's DAG provisions them concurrently,
# so the runner does a single apply instead of fanning out N terraform dirs.
resource "aws_instance" "e2e" {
  for_each = var.oses

  ami                    = local.ami_id[each.key]
  instance_type          = var.instance_type
  key_name               = aws_key_pair.e2e.key_name
  vpc_security_group_ids = [aws_security_group.e2e.id]
  user_data = templatefile("${path.module}/user_data.sh", {
    login_user = local.ssh_user[each.key]
    # dnf5daemon-server (the org.rpm.dnf.v0 provider the `packages` capability
    # drives) ships on Fedora 41+ but NOT on RHEL 10 (RHEL 10 is a dnf4 stack;
    # the package does not exist in BaseOS/AppStream/CRB). Installing it under
    # `set -e` would abort cloud-init before the ready marker, killing the whole
    # host over a backend fez is designed to gracefully skip (exit 9). Omit it
    # on RHEL; the `packages` e2e test then hits the dependency-missing path and
    # records `skip`, while services/network/firewall still run.
    with_dnf5daemon = each.key != "rhel10"
  })
  tags = merge(local.tags, { Name = "fez-e2e-${each.key}", OS = each.key })

  # Terraform owns the readiness wait. The provisioner SSHes in over the
  # generated key and blocks until cloud-init writes /var/lib/fez-e2e-ready,
  # bounded by var.ready_timeout_seconds so a host that never readies fails
  # THIS instance (taint) without hanging the apply forever (issue #49 moved
  # from a hand-rolled shell probe into a single hard timeout here).
  connection {
    type        = "ssh"
    host        = self.public_ip
    user        = local.ssh_user[each.key]
    private_key = tls_private_key.e2e.private_key_openssh
    timeout     = "${var.ready_timeout_seconds}s"
  }
  provisioner "remote-exec" {
    inline = [
      "until test -f /var/lib/fez-e2e-ready; do sleep 5; done",
    ]
  }

  lifecycle {
    precondition {
      # Only the Fedora auto-selection can drift onto a prerelease build; the
      # RHEL Hourly2 glob already excludes betas. Skip the check when a pin is
      # set or fedora is not in the requested set.
      condition     = !contains(var.oses, "fedora") || var.fedora_ami_id != null || !can(regex("(?i)prerelease|beta|rawhide|eln|rc", data.aws_ami.fedora.name))
      error_message = "Auto-selected Fedora AMI '${data.aws_ami.fedora.name}' looks like a pre-release; pin var.fedora_ami_id."
    }
  }
}
