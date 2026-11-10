# AMBAR OS — Master Development Plan

## 1. Product Vision

Build Ambar OS, a modern, AI-native operating system based on Linux.

**Product philosophy:**
- The beauty and simplicity of macOS.
- The openness and flexibility of Linux.
- The intelligence and automation capabilities of the AI era.
- Strong security and privacy by default.

Tagline: "Designed for Humans. Built for AI. Open to Everyone."

## 2. Core Product Modules

### A. Ambar Desktop
- Premium, minimalist desktop interface.
- Light and dark themes.
- Warm amber/saffron accent color.
- Application dock and launcher.
- File manager and terminal.
- Notifications and quick settings.
- Window management and workspaces.
- Global search and AI assistant.

### B. Ambar Intelligence
- Integrated AI assistant.
- Support multiple LLM providers.
- Local and cloud model support.
- Agent creation and management.
- Agent memory and orchestration.
- MCP and tool integrations.
- Agent execution monitoring.

### C. Ambar Automation
This is a core product feature, not an optional add-on.

Users should automate daily tasks using natural language.

Examples:
- Organize files automatically.
- Summarize emails and prepare drafts.
- Create daily reports.
- Manage documents and invoices.
- Automate development workflows.
- Schedule recurring tasks.
- Monitor selected folders and applications.

Provide three automation interfaces:
1. Natural-language AI automation builder.
2. Visual drag-and-drop workflow builder.
3. Developer SDK and API.

Every workflow must support triggers, actions, conditions, scheduling, execution history, pause, resume and cancellation.

Require appropriate user permissions and confirmation for sensitive actions.

### D. Ambar Security
- Agent identity and permissions.
- Sandboxed execution.
- Application permission management.
- Security monitoring and audit logs.
- Secrets management.
- Human approval for sensitive operations.
- Secure automation execution.

### E. Ambar Developer Ecosystem
- Public SDK and APIs.
- Application development framework.
- Agent development SDK.
- Plugin and extension framework.
- CLI.
- Application marketplace.
- Developer documentation.
- Community contribution support.

## 3. Technology Foundation

Use a modern, maintainable technology stack.

- Base OS: Fedora-based Linux.
- OS image: bootc.
- Kernel: Linux LTS.
- System programming: Rust.
- Desktop: KDE Plasma initially, customized with Ambar branding.
- UI: Qt/QML for desktop components.
- Automation engine: Rust.
- AI integrations: Python/Rust/TypeScript as appropriate.
- Database: SQLite locally, PostgreSQL for enterprise services.
- Containers: Podman.
- Virtualization: KVM/QEMU.
- Security: SELinux and sandboxed execution.
- Observability: OpenTelemetry.

Verify actual stable versions and compatibility before selecting dependencies. Avoid abandoned packages and unnecessary experimental dependencies.

## 4. Development Phases

### Phase 1 — Foundation
- Inspect the existing repository.
- Establish clean architecture.
- Configure build systems.
- Set up CI and automated testing.
- Implement initial Rust workspace.

### Phase 2 — Automation Engine
- Workflow definitions and validation.
- Trigger and scheduler system.
- Action execution engine.
- File-system integrations.
- Notifications.
- Execution history.
- Cancellation and error handling.
- Permission enforcement.

### Phase 3 — AI Integration
- Natural-language workflow generation.
- LLM provider abstraction.
- Model configuration.
- Workflow preview and approval.
- AI-powered automation suggestions.

### Phase 4 — Agent Runtime
- Agent registry.
- Agent identity.
- Isolated execution.
- Memory management.
- Tool permissions.
- Monitoring and auditing.

### Phase 5 — Ambar Desktop
- Custom desktop experience.
- Ambar launcher.
- AI assistant.
- Automation center.
- System settings.
- Application manager.

### Phase 6 — Developer Ecosystem
- SDK and CLI.
- Plugin architecture.
- Application packaging.
- Extension marketplace.
- Documentation.

### Phase 7 — Bootable OS
- Fedora-based bootable image.
- Ambar branding.
- Installation process.
- Hardware compatibility.
- Secure updates.
- System recovery and rollback.
- QEMU and physical hardware testing.

## 5. Engineering Requirements

- Modular architecture.
- Clear separation of system services.
- Strict type safety.
- Automated unit and integration tests.
- Secure-by-default permissions.
- No unrestricted AI access to the host.
- No arbitrary AI-generated shell execution without explicit authorization.
- Comprehensive error handling.
- Structured logging.
- Reproducible builds.
- Dependency and vulnerability scanning.
- Documentation for every major module.

## 6. Immediate Objective

Do NOT attempt to build every feature simultaneously.

Start with Phase 1 and Phase 2.

The first deliverable must be a functioning local automation engine that can:

1. Create and validate a workflow.
2. Execute a scheduled workflow.
3. Monitor a selected folder.
4. Perform approved file operations.
5. Send a notification.
6. Record execution history.
7. Pause and cancel workflows.
8. Enforce permissions.

Create automated tests for these capabilities.

## 7. Development Instructions

Before writing code:

1. Inspect the complete existing repository.
2. Identify implemented components and missing components.
3. Review existing dependencies and architecture.
4. Prepare an implementation plan.
5. Identify technical risks.

Then implement incrementally.

After every milestone:
- Run formatting and linting.
- Run unit and integration tests.
- Fix errors.
- Update documentation.
- Report completed functionality and remaining work.

Never claim a feature is implemented unless it actually works and has been tested.

**Final objective:** Build Ambar OS into a beautiful, open, secure and customizable AI-native operating system that makes everyday computing and automation easier for everyone.