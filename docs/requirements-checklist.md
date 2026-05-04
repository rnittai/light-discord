# Requirements Checklist

Confirm these before turning the scaffold into a production Discord-like app.

1. Deployment model: self-hosted server, managed cloud service, or LAN-only.
2. Identity: anonymous display names, local accounts, OAuth, SSO, or passkeys.
3. Server model: one server only, Discord-like guilds, invite links, roles, and permissions.
4. Persistence: PostgreSQL for real deployments, memory backend only for local development, visible message retention capped at 30 days.
5. Voice quality: Opus codec target bitrate, mono/stereo, push-to-talk, echo cancellation, noise suppression, input gain. Native input/output device selection exists in the MVP client.
6. Network topology: direct UDP relay, SFU-like relay, NAT traversal, TURN fallback, encryption requirements.
7. Security: TLS, end-to-end encryption, abuse prevention, rate limits, audit logs.
8. Moderation: block/report users, mute/deafen/kick/ban, channel permissions.
9. File/media: attachments, images, link previews, maximum file size, malware scanning.
10. Notifications: native notifications, unread badges, tray behavior, do-not-disturb.
11. Accessibility: keyboard navigation, screen reader labels, contrast, font scaling.
12. Packaging: Windows MSI/portable zip, Linux AppImage/deb/rpm, auto-update policy.
13. Compatibility: minimum Windows version, target Linux distributions, Wayland/X11 support.
14. Operations: logging, metrics, backups, migration process, crash reporting.
15. Audit: deleted messages are stored in admin-only audit logs with body snapshots; retention and legal policy must be explicit before hosted launch.
