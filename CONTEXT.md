# Newt

Newt is an offline reading device with a small collection of useful pauses.

Today Newt ships built-in apps; Pocket Pause is the working prototype. Newt is
heading toward apps anyone can make and share. That design separates a
shareable app from the recipient's decision to install it, and the device's
installed state from the control plane's acknowledgement. Neither sharing nor a
control plane exists yet.

## Language

### Reading and Home

**Home**:
The starting place for a current book, pauses, apps and the reader itself.
_Avoid_: Dashboard, launcher

**Home section**:
One of Home's three named destinations: Books, Apps or Settings.
_Avoid_: Home item, screen, experience

**Home section list**:
The group of all three Home sections.
_Avoid_: Morning Page area, navigation app

**Books**:
The Home section for reading material and the current book.
_Avoid_: Library when referring to ChatGPT Library

**Apps**:
The Home section for the reader's small offline apps.
_Avoid_: Morning Page, device settings

**Settings**:
The Home section and page for whole-device choices, such as the Home clock's
time zone. Diagnostics is inside it.
_Avoid_: Your X3, Your little device, Your little Newt

**Pocket Pause**:
The app offering a breath, a little movement or a drink of water.
_Avoid_: Home navigation, Morning Page

**Pause**:
One enabled Pocket Pause experience, selected for the reader to try now.
_Avoid_: Navigation card, menu destination

**Pause prompt**:
The readable instruction for a pause.
_Avoid_: Note when the text is specific to one pause

**Current book**:
The book the reader most recently opened and can continue reading.
_Avoid_: Saved page, Morning Page

**Reading position**:
The place in a book to which the reader returns after leaving or sleeping.
_Avoid_: Home focus, app selection

**Wake destination**:
The reading page when a book is open at sleep; Home otherwise.
_Avoid_: Previous screen, saved menu, app resume

**Sleep screen**:
The frosted wake destination with the approved Newt logo clearly above it.
_Avoid_: Last framebuffer, arbitrary screensaver

### Device

**Salamander**:
Newt's device layer: the code that talks to the X3's SDK and hardware, and the
facts about the device, such as its screen size.
_Avoid_: HAL, board (for the whole layer)

### Apps and delivery

**App**:
A small useful experience described by content and trusted semantic actions.
_Avoid_: Guest program, arbitrary device script

**App artifact**:
An immutable portable edition of an app that can be shared independently of any recipient or device.
_Avoid_: Deployment, device queue entry

**Artifact identity**:
The identity of the exact shared app content, unchanged by who receives it.
_Avoid_: Desired revision, device identity

**Registry**:
The owner of app artifact discovery and share references.
_Avoid_: Recipient control plane

**Control plane**:
The owner's desired app state for their devices and their delivery acknowledgements.
_Avoid_: Registry, universal app identity

**Desired state**:
A recipient's intent that a particular device activate a particular app artifact.
_Avoid_: Share, confirmed installation

**Desired revision**:
The ordering of one device's successive recipient intents.
_Avoid_: App version

**Installed receipt**:
The device's evidence that a particular desired revision and its app content were activated together.
_Avoid_: Queued intent, download completion

**Acknowledgement**:
The control plane's knowledge of a device's installed receipt.
_Avoid_: Atomic installation
