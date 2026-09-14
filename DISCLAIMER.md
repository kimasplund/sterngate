# LEGAL DISCLAIMER AND LIMITATION OF LIABILITY

> [!CAUTION]
> **CRITICAL NOTICE: READ CAREFULLY BEFORE USING THIS SOFTWARE**
>
> **STERNGATE COMMUNICATES DIRECTLY WITH AUTOMOTIVE CONTROLLERS, ENGINE MANAGEMENT SYSTEMS, POWERTRAIN COMPUTERS, HYDRAULICS, AND SAFETY-CRITICAL BRAKING HARDWARE. IMPROPER USE, COMMUNICATION DROPOUTS, POWER LOSS, OR MISCONFIGURATION CAN CAUSE PERMANENT ECU FAILURE ("BRICKING"), VEHICLE IMMOBILIZATION, MECHANICAL DAMAGE, PERSONAL INJURY, OR DEATH.**

---

## 1. Use Entirely at Your Own Risk

**Sterngate is provided "as is", without warranty of any kind, express or implied.** By downloading, compiling, running, or interacting with Sterngate in any form (via the command line, web dashboard, REST API, WebSocket streams, P2P remote tunnels, or Model Context Protocol AI agents), **you explicitly acknowledge and agree that you are using this software entirely at your own risk**.

---

## 2. Absolute Limitation of Liability

TO THE MAXIMUM EXTENT PERMITTED BY APPLICABLE LAW:

1. **No Liability for Bricked ECUs or Vehicles**: UNDER NO CIRCUMSTANCES SHALL THE AUTHORS, COPYRIGHT HOLDERS, CONTRIBUTORS, OR DISTRIBUTORS OF STERNGATE BE HELD LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, PUNITIVE, OR CONSEQUENTIAL DAMAGES ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE. This includes, but is not limited to:
   - Permanently damaged, corrupted, or "bricked" Electronic Control Units (ECUs), Transmission Control Modules (TCMs), Central Gateways (CGWs), or other microcontrollers;
   - Vehicle immobilization, tow truck fees, dealer reprogramming costs, or replacement hardware expenses;
   - Mechanical failures, engine hydrolock, fuel system starvation, transmission slippage, hydraulic burst, air suspension collapse, or compressor burnout;
   - Brake system deactivation or failure (including Sensotronic Brake Control / SBC service routines);
   - Traffic accidents, collisions, physical injury, or loss of life;
   - Voided vehicle manufacturer warranties, emissions non-compliance, inspection failures, or legal fines.

2. **No Warranty of Fitness or Accuracy**: The authors make no representations or warranties regarding the accuracy, completeness, safety, or reliability of any diagnostic data, Parameter Identifiers (DIDs), DTC definitions, variant coding bits, seed-key solving algorithms, flash routines, or vehicle profiles provided in this repository.

---

## 3. High-Risk Operational Warnings

If you use Sterngate to perform variant coding, routine actuations, or firmware flashing:

- **Stable Power Supply Mandatory**: Never attempt an ECU flash or critical routine without a commercial, voltage-stabilized automotive power supply / battery maintainer capable of sustaining $\ge 12.5\text{ V}$ at $\ge 25\text{–}50\text{ A}$. Standard battery chargers and booster packs are NOT voltage-regulated and can cause fatal voltage ripples.
- **SBC High-Pressure Hazard**: Sensotronic Brake Control operates at pressures up to **160 bar (2,300 psi)**. Working on SBC calipers without verified hydraulic deactivation can cause severe crushing injuries or amputation from unexpected high-pressure piston extension.
- **Bootloader Vulnerability**: Interrupted erase or write operations (`0x31 Routine 0xFF00` / `0x36 TransferData`) can leave an ECU in an unrecoverable bootloader or blank ROM state that requires desoldering flash memory chips or BDM/JTAG bench recovery.

---

## 4. Independent Project & Trademark Disclaimers

- **Independent Clean-Room Project**: Sterngate is an independent, open-source research and engineering software project.
- **Trademarks**: All brand names, vehicle makes, module acronyms, and product names (including Mercedes-Benz, Daimler, AMG, Bosch, Siemens, Continental, Tactrix, OpenPort, EcuFlash, Vediamo, Xentry, DAS, and Star Diagnosis) are trademarks or registered trademarks of their respective holders.
- **No Affiliation**: Sterngate, its authors, and its contributors are **not affiliated with, authorized by, sponsored by, or endorsed by** Mercedes-Benz AG, Mercedes-Benz Group AG, Robert Bosch GmbH, Tactrix Inc., or any other automotive manufacturer or supplier.

---

## 5. Acceptance of Terms

**IF YOU DO NOT AGREE TO THESE TERMS, DO NOT CONNECT THIS SOFTWARE TO ANY MOTOR VEHICLE OR AUTOMOTIVE CONTROLLER. REMOVE AND DELETE ALL COPIES OF THIS SOFTWARE IMMEDIATELY.**

