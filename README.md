<div align="center">
  <img src="https://github.com/BICH0/eoka/assets/81905574/2501bd8d-2b1e-4ebf-bfb2-5d8db11a88be" alt="Logo" height="320">  

  ### Eoka  
  
  Linux package manager made with Rust!! It uses its own mirror servers and package format, for more info about this head to the [Autoturret GitHub (Mirror structure)](https://github.com/BICH0/Autoturret)
  This project forms part of the CNFOs project, its currently in a ver primitive state and development is not expected early but feel free to peak on the code and use it in whichever form you wish (Complying with the License this currently under)
  
  [**Explore the docs »**](https://github.com/BiCH0/eoka)  
  [Live Demo](https://github.com/BiCH0/eoka/#Demo) · [Report Bug](https://github.com/BiCH0/eoka/issues) · [Request Feature](https://github.com/BiCH0/eoka/issues)
  
</div>

# Index
* ### [Requirements](#-requirements)
* ### [Installation](#-installation)
* ### [Usage](#-usage)
* ### [License](#-license)

# 💻 Requirements
Eoka is a compiled piece of software so if you just want to install it you won't need any special programs or utilities
In case eoka doesn't support your CPU architecture you can compile it from source following the steps of the [Building section](#-building)
# 🚀 Installation
To install eoka download the latest binary for your architecture from [releases section](https://github.com/BiCH0/eoka/releases/latest)  
In case you don't know which architecture you have, use the following command:  
```
uname -r
```  
**!! Its preferred that the owner of the eoka bin is set to UID 0 and GUID 0 (root:root) this is in order to prevent possible installation errors due to lack of privileges, if you know what you're doing change it to your likings, if not, use the following command to set it so:**  
```
chown root:root eoka-<version>.<arch>
```  
Once downloaded you just need to make the binary executable and place it somewhere in your PATH global variable, for example /usr/sbin  
```
chmod u+x eoka-<version>.<arch>
mv eoka-<version>.<arch> /usr/sbin/eoka
```  
And its all done, execute it and eoka will create all the necesary files/directories if needed.

# 🔧 Building
In order to build eoka for your system you will need the following tools:
* Rust
* Cargo
  
To install them in case you dont have already installed them issue this command:  
```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
This will install rust and cargo on your system, refer to the [rust installation web](https://www.rust-lang.org/tools/install) for more info  
Once you have installed the requirements you will need to actually build eoka:
1. Clone the repo
```
git clone https://github.com/BICH0/eoka.git
```
2. Set your working dir into the repo's directory
```
cd eoka
```   
3. Build eoka!! (WARNING THIS MAY TAKE A WHILE DEPENDING ON YOUR HARDWARE CONFIGURATION, IT SHOULDN'T BE MORE THAN 10 MINS BUT BE AWARE)
```
cargo build --release
```
4. Once the building process ends you will end with the eoka binary located in target/release, now you can follow the steps in the [installation section](#-installation)
# ☕ Usage

# 📜 License
This project is made under the GPLv3 license, refer to the [License]() for more info  
## LICENSE SYNOPSYS
1. Anyone can copy, modify and distribute this software.
2. You have to include the license and copyright notice with each and every distribution.
3. You can use this software privately.
4. You can use this software for commercial purposes.
5. If you dare build your business solely from this code, you risk open-sourcing the whole code base.
6. If you modify it, you have to indicate changes made to the code.
7. Any modifications of this code base MUST be distributed with the same license, GPLv3.
8. This software is provided without warranty.
9. The software author or license can not be held liable for any damages inflicted by the software.


<img src="https://upload.wikimedia.org/wikipedia/commons/thumb/9/93/GPLv3_Logo.svg/2560px-GPLv3_Logo.svg.png" width="80" height="15" alt="WTFPL" /></a>
