// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;
/// @notice Local test fixture only. NEVER label this token as Circle-issued USDC.
contract TestUSDC {
    uint8 public constant decimals = 6;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    function mint(address to, uint256 amount) external { balanceOf[to] += amount; }
    function approve(address to, uint256 amount) external returns (bool) { allowance[msg.sender][to] = amount; return true; }
    function transfer(address to, uint256 amount) external returns (bool) { move(msg.sender, to, amount); return true; }
    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        require(allowance[from][msg.sender] >= amount, "allowance");
        allowance[from][msg.sender] -= amount; move(from, to, amount); return true;
    }
    function move(address from, address to, uint256 amount) internal {
        require(balanceOf[from] >= amount, "balance"); balanceOf[from] -= amount; balanceOf[to] += amount;
    }
}
