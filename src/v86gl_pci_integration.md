# v86gl custom virtio device

`v86gl_pci.js` now wraps the existing `VirtIO` implementation. The custom PCI
register/doorbell transport has been replaced by feature negotiation and a
split virtqueue. The JavaScript option name is retained for source compatibility.

See [virtio-v86gl protocol and migration](../docs/v86gl-virtio.md) for device
identity, queue layouts, shared-arena lifetime, guest-driver deployment and tests.
See [browser graphics integration](../docs/glbridge.md) for embedding.

Old `v86gl.sys` binaries and old PCI-device memory snapshots are not compatible;
install the updated driver and cold-boot the disk.
